const globals = require("globals");
const pluginJs = require("@eslint/js");
const tseslint = require("typescript-eslint");
const stylistic = require("@stylistic/eslint-plugin");
const sort = require("eslint-plugin-sort");

const MAX_LINE_LENGTH = 120;
const INDENT_SIZE = 4;

function changeErrorToWarn(configObj) {
    for (const key in configObj) {
        if (configObj[key] === "error") {
            configObj[key] = "warn";
        } else if (typeof configObj[key] === "object") {
            const nestedConfigObj = configObj[key];
            for (const key in nestedConfigObj) {
                if (nestedConfigObj[key] === "error") {
                    nestedConfigObj[key] = "warn";
                }
            }
        }
    }
    return configObj;
}

const stylisticRecommendedConfig = stylistic.configs.customize({
    indent: INDENT_SIZE,
    quotes: "double",
    semi: true,
    jsx: false,
    arrowParens: true,
    commaDangle: "always-multiline",
});
changeErrorToWarn(stylisticRecommendedConfig.rules);

module.exports = [
    // --------- JavaScript and TypeScript files config ----------- //

    // Common ignored files (except the 'node_modules' folder that is ignored by default)
    {
        ignores: ["cache/**", "artifacts/**", "typechain-types/**"],
    },

    // Common recommended config
    {
        name: "common",
        files: ["**/*.js", "**/*.ts", "**/*.mjs", "**/*.mts", "**/*.cjs", "**/*.cts"],
        languageOptions: { globals: globals.node },
        ...pluginJs.configs.recommended,
    },

    // Stylistic recommended config
    {
        name: "script-stylistic",
        files: ["**/*.js", "**/*.ts", "**/*.mjs", "**/*.mts", "**/*.cjs", "**/*.cts"],
        ...stylisticRecommendedConfig,
    },

    // Sorting recommended config
    {
        plugins: sort.configs["flat/recommended"].plugins,
    },

    // ----------------- TypeScript only files config ------------------ //

    // TypeScript recommended config
    ...tseslint.configs.recommended.map((config) => ({
        ...config,
        files: ["**/*.ts", "**/*.mts", "**/*.cts"],
    })),

    // Typescript special rules
    {
        name: "type-script-base-special-rules",
        files: ["**/*.ts", "**/*.mts", "**/*.cts"],
        rules: {
            "@typescript-eslint/no-explicit-any": "off", // Allow to use `any`
            "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
        },
    },

    // ----------------- Special rules ------------------ //

    {
        name: "special",
        files: ["**/*.js", "**/*.ts", "**/*.mjs", "**/*.mts", "**/*.cjs", "**/*.cts"],
        rules: {
            // Common rules
            "no-constant-condition": ["error", { checkLoops: false }],

            // Stylistic rules
            "@stylistic/brace-style": ["warn", "1tbs", { allowSingleLine: true }],
            "@stylistic/max-len": ["warn", { code: MAX_LINE_LENGTH }],
            "@stylistic/operator-linebreak": ["warn", "after", { overrides: { "?": "before", ":": "before" } }],

            // Sorting rules
            "sort/imports": [
                "warn",
                {
                    groups: [
                        { type: "dependency", order: 10 },
                        { type: "other", order: 20 },
                    ],
                    separator: "\n",
                    caseSensitive: true,
                },
            ],
            "sort/import-members": ["warn", { caseSensitive: true }],
        },
    },
];
