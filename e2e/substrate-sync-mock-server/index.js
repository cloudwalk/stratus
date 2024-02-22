const express = require('express');
const fs = require('fs');
const { parse } = require('csv-parse/sync'); // Ensure to install csv-parse
const app = express();
const port = 3003;

// Load and parse CSV data synchronously
const csvFilePath = './e2e/substrate-sync-mock-server/stratus_test_data.csv';
const csvContent = fs.readFileSync(csvFilePath, 'utf-8');
const csvData = parse(csvContent, {
    columns: true,
    skip_empty_lines: true
});

app.use(express.json());

console.log('Loaded', csvData.length, 'rows of data from', csvFilePath);
app.post('/rpc', (req, res) => {
    console.log('Received RPC request:', req.body);
    try {
        if (req.body.method === 'eth_getBlockByNumber') {
            const blockNumberHex = req.body.params[0];
            const blockNumber = parseInt(blockNumberHex, 16);

            console.log('Requested block number:', blockNumber);
                const matchingBlock = csvData.find(row => (row.block_number-1) === blockNumber);
            if (matchingBlock && matchingBlock.block_payload) {
                res.json({
                    jsonrpc: "2.0",
                    result: JSON.parse(matchingBlock.block_payload),
                    id: req.body.id
                });
            } else {
                res.status(404).json({ error: "Block not found" });
            }
        } else if (req.body.method === 'eth_getTransactionReceipt') {
            const txHash = req.body.params[0];

            const matchingReceipt = csvData.find(row => ('0x' + row.transaction_hash) === txHash);
            if (matchingReceipt && matchingReceipt.receipt_payload) {
                res.json({
                    jsonrpc: "2.0",
                    result: JSON.parse(matchingReceipt.receipt_payload),
                    id: req.body.id
                });
            } else {
                res.status(404).json({ error: "Receipt not found" });
            }
        } else if (req.body.method === 'eth_blockNumber') {
            // Handler for 'eth_blockNumber'
            const latestBlock = csvData.reduce((max, row) => Math.max(max, row.block_number), 0);
            res.json({
                jsonrpc: "2.0",
                result: `0x${latestBlock.toString(16)}`, // Convert to hex string
                id: req.body.id
            });
        }
    } catch (error) {
        console.error('Error processing request:', error);
        res.status(500).json({ error: "Internal server error" });
    }
});

app.listen(port, () => {
    console.log(`Server listening at http://localhost:${port}`);
});
