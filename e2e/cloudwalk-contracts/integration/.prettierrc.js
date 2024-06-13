module.exports = {
    plugins: ["@trivago/prettier-plugin-sort-imports"],
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
};
