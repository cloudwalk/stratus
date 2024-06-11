module.exports = {
    plugins: ["@trivago/prettier-plugin-sort-imports", "prettier-plugin-solidity"],
    // lines
    printWidth: 120,
    tabWidth: 4,
    singleQuote: false,
    trailingComma: "all",
    semi: true,

    // attributes
    singleAttributePerLine: true,

    // imports
    importOrder: ["<THIRD_PARTY_MODULES>", "^[./]"],
    importOrderSeparation: true,
    importOrderSortSpecifiers: true,

    overrides: [
        // Solidity
        {
            files: "*.sol",
            options: {
                parser: "solidity-parse",
                printWidth: 120,
                tabWidth: 4,
                useTabs: false,
                singleQuote: false,
                bracketSpacing: true,
            },
        },
    ],
};
