use core::panic;
use std::collections::HashMap;
use std::collections::HashSet;

use itertools::Itertools;
use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use petgraph::visit::IntoNodeIdentifiers;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Index;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

pub struct TransactionDag {
    dag: StableGraph<TransactionMined, i32>,
}

impl TransactionDag {
    pub fn get_slot_writes(block_transactions: &[TransactionMined]) -> HashSet<(Address, SlotIndex)> {
        block_transactions
            .iter()
            .flat_map(|tx| {
                tx.execution.changes.iter().flat_map(|(address, change)| {
                    change
                        .slots
                        .iter()
                        .filter_map(|(idx, slot_change)| slot_change.is_modified().then_some((*address, *idx)))
                })
            })
            .collect()
    }

    pub fn txs_remaining(&self) -> usize {
        self.dag.node_count()
    }

    pub fn get_balance_writes(block_transactions: &[TransactionMined]) -> HashSet<Address> {
        block_transactions
            .iter()
            .flat_map(|tx| {
                tx.execution
                    .changes
                    .iter()
                    .filter_map(|(address, change)| change.balance.is_modified().then_some(*address))
            })
            .collect()
    }

    /// Uses the transactions and produces a Dependency DAG (Directed Acyclical Graph).
    /// Each vertex of the graph is a transaction, and two vertices are connected iff they conflict
    /// on either a slot or balance and they don't have the same "from" field (since those transactions will
    /// be ordered by nonce).
    /// The direction of an edge connecting the transactions A and B is always from
    /// `min(A.transaction_index, B.transaction_index)` to `max(A.transaction_index, B.transaction_index)`.
    /// This combined with the fact transactions indexes are unique makes it impossible for a cycle to be inserted.
    ///
    /// Proof:
    /// Assume that it was the case that a cycle $C = (v_1, v_2, ..., v_n, v_1)$ was inserted, since we only insert edges from
    /// $min(TransactionIndex(A), TransactionIndex(B))$ to $max(TransactionIndex(A), TransactionIndex(B))$ and
    /// $A != B \iff TransactionIndex(A) != TransactionIndex(B)$ then $TransactionIndex(v_i) < TransactionIndex(v_{i+1})$ it
    /// follows that $TransactionIndex(v_1) < TransactionIndex(v_2)$. Thus by induction and the transitive property of inequality
    /// $TransactionIndex(v_1) < TransactionIndex(v_n)$, and therefore there cannot be an edge going from $v_n$ to $v_1$. □
    ///
    /// Possible issues: There is a dependency between contract deployments and contract calls that is not taken into consideration.
    #[tracing::instrument(skip_all)]
    pub fn new(block_transactions: Vec<TransactionMined>) -> Self {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let slot_writes: HashSet<(Address, SlotIndex)> = Self::get_slot_writes(&block_transactions);
        let balance_writes: HashSet<Address> = Self::get_balance_writes(&block_transactions);

        let mut slot_conflicts: HashMap<(BlockNumber, Index), HashSet<(Address, SlotIndex)>> = HashMap::new();
        let mut balance_conflicts: HashMap<(BlockNumber, Index), HashSet<Address>> = HashMap::new();
        let mut node_indexes: HashMap<(BlockNumber, Index), NodeIndex> = HashMap::new();
        let mut dag = StableGraph::new();

        for tx in block_transactions.into_iter().sorted() {
            let tx_idx = tx.transaction_index;
            let tx_bnum = tx.block_number;
            for (address, change) in &tx.execution.changes {
                for idx in change.slots.keys() {
                    let slot = (*address, *idx);
                    if slot_writes.contains(&slot) {
                        slot_conflicts.entry((tx_bnum, tx_idx)).or_default().insert(slot);
                    }
                }

                let addr = *address;
                if balance_writes.contains(&addr) {
                    balance_conflicts.entry((tx_bnum, tx_idx)).or_default().insert(addr);
                }
            }
            let node_idx = dag.add_node(tx);
            node_indexes.insert((tx_bnum, tx_idx), node_idx);
        }

        Self::compute_edges(&mut dag, slot_conflicts, &node_indexes);
        Self::compute_edges(&mut dag, balance_conflicts, &node_indexes);

        #[cfg(feature = "metrics")]
        metrics::inc_compute_tx_dag(start.elapsed());

        Self { dag }
    }

    fn compute_edges<T: std::hash::Hash + std::cmp::Eq + serde::Serialize>(
        dag: &mut StableGraph<TransactionMined, i32>,
        conflicts: HashMap<(BlockNumber, Index), HashSet<T>>,
        node_indexes: &HashMap<(BlockNumber, Index), NodeIndex>,
    ) {
        for (i, (tx1, set1)) in conflicts.iter().sorted_by_key(|(idx, _)| **idx).enumerate() {
            let tx1_node_index = *node_indexes.get(tx1).unwrap();
            let tx1_from = dag.node_weight(tx1_node_index).unwrap().input.signer;
            for (tx2, set2) in conflicts.iter().sorted_by_key(|(idx, _)| **idx).skip(i + 1) {
                let tx2_node_index = *node_indexes.get(tx2).unwrap();
                let tx2_from = dag.node_weight(tx2_node_index).unwrap().input.signer;

                if tx1_from != tx2_from && !set1.is_disjoint(set2) {
                    tracing::debug!(?tx1, ?tx2, "adding edge");
                    dag.add_edge(*node_indexes.get(tx1).unwrap(), *node_indexes.get(tx2).unwrap(), 1);
                }
            }
        }
    }

    pub fn split_components(self) -> Vec<Self> {
        let mut dag = self.dag;
        let mut ret = vec![];

        while dag.node_count() > 0 {
            let mut root = None;

            // find a root
            for index in dag.node_identifiers() {
                if dag.neighbors_directed(index, petgraph::Direction::Incoming).next().is_none() {
                    root = Some(index);
                    break;
                }
            }

            let Some(root) = root else {
                panic!("cycle detected, transaction dag with more than 0 vertices but no roots")
            };

            // get the root's component vertices
            let mut current_component = HashSet::new();
            let mut current_component_queue = vec![root];
            while let Some(node) = current_component_queue.pop() {
                current_component.insert(node);
                for neighbor in dag.neighbors_undirected(node) {
                    if !current_component.contains(&neighbor) {
                        current_component_queue.push(neighbor);
                    }
                }
            }

            let mut transactions_in_component = vec![];
            for node in current_component {
                transactions_in_component.push(
                    dag.remove_node(node)
                        .expect("all the nodes were obtained in the previous step, and should still exists in the graph"),
                );
            }

            ret.push(Self::new(transactions_in_component));
        }
        ret
    }

    /// Takes the roots (vertices with no parents) from the DAG, removing them from the graph,
    /// and by extension creating new roots for a future call. Returns `None` if the graph
    /// is empty.
    #[tracing::instrument(skip_all)]
    pub fn take_roots(&mut self) -> Option<Vec<TransactionMined>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();
        let dag = &mut self.dag;

        let mut root_indexes = vec![];
        for index in dag.node_identifiers() {
            if dag.neighbors_directed(index, petgraph::Direction::Incoming).next().is_none() {
                root_indexes.push(index);
            }
        }

        let mut roots = vec![];
        while let Some(root) = root_indexes.pop() {
            roots.push(dag.remove_node(root).expect("removing a known vertex should not fail"));
        }

        #[cfg(feature = "metrics")]
        metrics::inc_take_roots(start.elapsed());

        if roots.is_empty() {
            None
        } else {
            Some(roots)
        }
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashSet;
    use std::io::BufReader;

    use fake::Fake;
    use fake::Faker;

    use super::TransactionDag;
    use crate::eth::primitives::Address;
    use crate::eth::primitives::Block;
    use crate::eth::primitives::Bytes;
    use crate::eth::primitives::CodeHash;
    use crate::eth::primitives::EvmExecution;
    use crate::eth::primitives::ExecutionAccountChanges;
    use crate::eth::primitives::ExecutionResult;
    use crate::eth::primitives::ExecutionValueChange;
    use crate::eth::primitives::Gas;
    use crate::eth::primitives::Hash;
    use crate::eth::primitives::Slot;
    use crate::eth::primitives::SlotIndex;
    use crate::eth::primitives::TransactionMined;
    use crate::eth::primitives::UnixTime;

    const ADDRESS: Address = Address::ZERO;

    fn create_tx(changed_slots_inidices: HashSet<SlotIndex>, block_number: u64, tx_idx: u64) -> TransactionMined {
        let execution_changes = ExecutionAccountChanges {
            new_account: false,
            address: ADDRESS,
            nonce: ExecutionValueChange::default(),
            balance: ExecutionValueChange::default(),
            bytecode: ExecutionValueChange::default(),
            code_hash: CodeHash::default(),
            slots: changed_slots_inidices
                .into_iter()
                .map(|index| (index, ExecutionValueChange::from_modified(Slot { index, value: 0.into() })))
                .collect(),
        };
        let execution = EvmExecution {
            block_timestamp: UnixTime::default(),
            receipt_applied: false,
            result: ExecutionResult::Success,
            output: Bytes::default(),
            logs: vec![],
            gas: Gas::default(),
            changes: [(ADDRESS, execution_changes)].into_iter().collect(),
            deployed_contract_address: None,
        };

        TransactionMined {
            input: Faker.fake(),
            execution,
            logs: vec![],
            transaction_index: tx_idx.into(),
            block_number: block_number.into(),
            block_hash: Hash::default(),
        }
    }

    #[test]
    fn test_real_tx() {
        let file = std::fs::File::open("./tests/fixtures/blocks/simple_dag_test.json").unwrap();
        let reader = BufReader::new(file);
        let block = serde_json::from_reader::<_, Block>(reader).unwrap();
        let mut dag = TransactionDag::new(block.transactions);

        let expected = [[0], [1], [2]];
        let mut i = 0;
        while let Some(roots) = dag.take_roots() {
            assert_eq!(roots.len(), expected[i].len());
            assert!(roots.iter().all(|tx| expected[i].contains(&tx.transaction_index.0)));
            i += 1;
        }
        //println!("{:?}", petgraph::dot::Dot::with_config(&dag.dag, &[petgraph::dot::Config::EdgeNoLabel, petgraph::dot::Config::NodeIndexLabel]));
    }

    #[test]
    fn test_split_components() {
        let expected_component_1 = vec![vec![0, 1], vec![2], vec![3], vec![4, 5], vec![6]];
        let expected_component_2 = vec![vec![7], vec![8, 9], vec![10, 11], vec![12, 13, 14, 15], vec![16]];
        let test = vec![
            // component 1
            vec![1],       // (0): component root
            vec![2],       // (1): component root
            vec![1, 2, 3], // (2): depends on (0) and (1)
            vec![3, 4, 5], // (3): depends on (2)
            vec![4, 7],    // (4): depends on (3)
            vec![3, 8],    // (5): depends on (3)
            vec![8, 7],    // (6): depends on (4) and (5)
            // component 2
            vec![9, 10],          // (7): component root
            vec![9, 11],          // (8): depends on (7)
            vec![10, 15],         // (9): depends on (7)
            vec![11, 12, 13],     // (10): depends on (8)
            vec![15, 16, 17],     // (11): depends on (9)
            vec![12, 18],         // (12): depends on (10)
            vec![13, 19],         // (13): depends on (10)
            vec![16, 20],         // (14): depends on (11)
            vec![17, 21],         // (15): depends on (11)
            vec![18, 19, 20, 21], // (17): depends on (12), (13), (14) and (15)
        ];

        let transactions = test
            .into_iter()
            .map(|indexes| indexes.into_iter().map(SlotIndex::from))
            .enumerate()
            .map(|(i, indexes)| create_tx(indexes.collect(), i as u64, i as u64))
            .collect();

        let dag = TransactionDag::new(transactions);
        let components = dag.split_components();
        assert_eq!(components.len(), 2);

        for mut component in components {
            if component.dag.node_count() == 7 {
                let mut i = 0;
                let expected = expected_component_1.clone();
                while let Some(roots) = component.take_roots() {
                    assert_eq!(roots.len(), expected[i].len());
                    assert!(roots.iter().all(|tx| expected[i].contains(&tx.transaction_index.0)));
                    i += 1;
                }
                continue;
            }

            if component.dag.node_count() == 10 {
                let expected = expected_component_2.clone();
                let mut i = 0;
                while let Some(roots) = component.take_roots() {
                    assert_eq!(roots.len(), expected[i].len());
                    assert!(roots.iter().all(|tx| expected[i].contains(&tx.transaction_index.0)));
                    i += 1;
                }
                continue;
            }
            panic!("unreachable")
        }
    }

    #[test]
    fn test_compute_tx_dag_and_take_roots() {
        let expected1 = vec![vec![0, 1], vec![2], vec![3], vec![4, 5], vec![6]];
        let transactions1 = vec![
            vec![1],       // (0): dag root
            vec![2],       // (1): dag root
            vec![1, 2, 3], // (2): depends on (0) and (1)
            vec![3, 4, 5], // (3): depends on (2)
            vec![4, 7],    // (4): depends on (3)
            vec![3, 8],    // (5): depends on (3)
            vec![8, 7],    // (6): depends on (4) and (5)
        ];

        let expected2 = vec![vec![0], vec![1, 2], vec![3, 4], vec![5, 6, 7, 8], vec![9]];
        let transactions2 = vec![
            vec![1, 2],           // (0): dag root
            vec![1, 3],           // (1): depends on (0)
            vec![2, 7],           // (2): depends on (0)
            vec![3, 4, 5],        // (3): depends on (1)
            vec![7, 8, 9],        // (4): depends on (2)
            vec![4, 10],          // (5): depends on (3)
            vec![5, 11],          // (6): depends on (3)
            vec![8, 12],          // (7): depends on (4)
            vec![9, 13],          // (8): depends on (4)
            vec![10, 11, 12, 13], // (9): depends on (5), (6), (7) and (8)
        ];

        let expected3 = vec![vec![0, 2, 3], vec![1], vec![4], vec![5, 7], vec![6, 10], vec![8, 11], vec![9]];
        let transactions3 = vec![
            vec![1],                  // (0): dag root
            vec![1, 2, 3],            // (1): depends on (0)
            vec![13],                 // (2): dag root
            vec![14, 15],             // (3): dag root
            vec![2, 4, 5, 6, 13, 14], // (4): depends on (2) and (3)
            vec![4, 12, 15, 16],      // (5): depends on (3) and (4)
            vec![5, 9, 16],           // (6): depends on (4) and (5)
            vec![3, 6, 7, 10],        // (7): depends on (1) and (4),
            vec![9, 10, 11, 12],      // (8): depends on (5), (6) and (7)
            vec![11],                 // (9): depends on (8)
            vec![7, 8],               // (10): depends on (7)
            vec![8],                  // (11): depends on (10)
        ];

        let tests = [transactions1, transactions2, transactions3];
        let expected_results = [expected1, expected2, expected3];

        for (test, expected) in tests.into_iter().zip(expected_results) {
            let transactions = test
                .into_iter()
                .map(|indexes| indexes.into_iter().map(SlotIndex::from))
                .enumerate()
                .map(|(i, indexes)| create_tx(indexes.collect(), i as u64, i as u64))
                .collect();

            let mut dag = TransactionDag::new(transactions);
            let mut i = 0;
            while let Some(roots) = dag.take_roots() {
                assert_eq!(roots.len(), expected[i].len());

                assert!(roots.iter().all(|tx| expected[i].contains(&tx.transaction_index.0)));
                i += 1;
            }
        }
    }
}
