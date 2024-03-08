const express = require('express');
const fs = require('fs');
const { parse } = require('csv-parse/sync'); // Ensure to install csv-parse
const app = express();
const port = 3003;

// Load and parse CSV data synchronously
const csvFilePath = './e2e/rpc-mock-server/stratus_test_data.csv';
const csvContent = fs.readFileSync(csvFilePath, 'utf-8');
const csvData = parse(csvContent, {
    columns: true,
    skip_empty_lines: true
});
for (const row of csvData) {
    row.block_number = parseInt(row.block_number);
}
csvData.sort((a, b) => a.block_number - b.block_number);

app.use(express.json());

console.log('Loaded', csvData.length, 'rows of data from', csvFilePath);
app.post('/rpc', (req, res) => {
    console.log('Received RPC request:', req.body);
    try {
        const method = req.body.method;
        const response = {
            jsonrpc: "2.0",
            id: req.body.id
        };

        switch(method) {
            case "net_listening":
                res.json({...response, result: true});
                break;

            case "eth_getBlockByNumber":
                const blockNumberHex = req.body.params[0];
                const blockNumber = parseInt(blockNumberHex, 16);

                console.log('Requested block number:', blockNumber);
                const matchingBlock = csvData.find(row => row.block_number === blockNumber);
                if (matchingBlock && matchingBlock.block_payload) {
                    console.log("Block found");
                    res.json({...response, result: JSON.parse(matchingBlock.block_payload)});
                } else {
                    console.log("Block not found");
                    res.json({...response, result: null})
                }
                break;

            case "eth_getTransactionReceipt":
                const txHash = req.body.params[0];

                const matchingReceipt = csvData.find(row => ('0x' + row.transaction_hash) === txHash);
                if (matchingReceipt && matchingReceipt.receipt_payload) {
                    console.log("Receipt found");
                    res.json({...response, result: JSON.parse(matchingReceipt.receipt_payload)})
                } else {
                    console.log("Receipt not found");
                    res.json({...response, result: null})
                }
                break;

            case "eth_blockNumber":
                const latestBlock = csvData.reduce((max, row) => Math.max(max, row.block_number), 0);
                res.json({...response, result: `0x${latestBlock.toString(16)}`});
                break;
        }
    } catch (error) {
        console.error('Error processing request:', error);
        res.status(500).json({ error: "Internal server error" });
    }
});

app.listen(port, () => {
    console.log(`Server listening at http://localhost:${port}`);
});
