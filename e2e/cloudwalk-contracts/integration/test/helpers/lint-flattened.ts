import fs from "fs";
import readline from "readline";

async function removeLinesContainingString(filePath: string, searchString: string[]) {
    const fileStream = fs.createReadStream(filePath);
    const rl = readline.createInterface({
        input: fileStream,
        crlfDelay: Infinity,
    });

    const newLines: string[] = [];
    newLines.push("// SPDX-License-Identifier: MIT");
    newLines.push("pragma solidity 0.8.24;");

    for await (const line of rl) {
        if (searchStrings.every((searchString) => !line.includes(searchString))) {
            newLines.push(line);
        }
    }

    fs.writeFileSync(filePath, newLines.join("\n").replace(/\n{3,}/g, "\n\n"));
}

const filePath = process.argv[2];
const searchStrings = [
    "pragma solidity",
    "Sources flattened",
    "SPDX-License-Identifier",
    "File @openzeppelin",
    "Original license",
    "OpenZeppelin Contracts",
    "@custom:storage-location",
];

removeLinesContainingString(filePath, searchStrings).catch((error) => console.error("Error:", error));
