const express = require('express');
const { Pool } = require('pg');
const app = express();
const port = 3003;

// Assuming connectionString is set via an environment variable or command line argument
const pool = new Pool({
    connectionString: process.env.DATABASE_URL,
});

app.use(express.json());

app.post('/rpc', async (req, res) => {
    try {
        // Extract block number from the request and convert from hex to integer
        const blockNumberHex = req.body.params[0];
        const blockNumber = parseInt(blockNumberHex, 16); // Convert hex to integer

        const query = `
            SELECT * FROM external_blocks
            WHERE external_blocks.number = $1
            LIMIT 1;
        `;

        // Execute the query, passing the block number as a parameter
        const result = await pool.query(query, [blockNumber.toString()]);

        if (result.rows.length > 0) {
            res.json(result.rows[0]);
        } else {
            console.error('No records found for block number:', blockNumber);
            process.exit(1);
        }
    } catch (error) {
        console.error('Error executing query', error.stack);
        process.exit(1);
    }
});

app.listen(port, () => {
    console.log(`Server listening at http://localhost:${port}`);
});
