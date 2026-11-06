"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

async function main() {
    const root = process.cwd();
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), "minimatch-compat-"));
    const ignored = path.join(temp, "ignored.test.cjs");
    const included = path.join(temp, "included.test.cjs");
    const source = path.join(temp, "coverage-target.js");

    try {
        fs.writeFileSync(ignored, "// must be ignored during collection\n");
        fs.writeFileSync(included, "// must be collected\n");
        fs.writeFileSync(source, "module.exports = true;\n");

        const mochaRoot = path.dirname(require.resolve("mocha", { paths: [root] }));
        const collectFiles = require(path.join(mochaRoot, "lib/cli/collect-files.cjs"));
        const collected = collectFiles({
            ignore: [ignored],
            extension: [".cjs"],
            file: [],
            recursive: true,
            sort: false,
            spec: [temp],
        });
        assert.deepEqual(collected.files, [included]);

        const fileMatcher = require(path.join(root, "node_modules/sc-istanbul/lib/util/file-matcher.js"));
        const files = await new Promise((resolve, reject) => {
            fileMatcher.filesFor(
                { root: temp, includes: ["**/*.js"], excludes: [], relative: true },
                (error, result) => (error ? reject(error) : resolve(result)),
            );
        });
        assert.deepEqual(files, ["coverage-target.js"]);

        const globbed = await require("globby")(["*.js"], { cwd: temp });
        assert.deepEqual(globbed, ["coverage-target.js"]);
        const shelljsFiles = require("shelljs")
            .ls(path.join(temp, "*.js"))
            .map((file) => path.basename(file));
        assert.deepEqual(shelljsFiles, ["coverage-target.js"]);

        console.log("Mocha, sc-istanbul, globby, and shelljs compatibility passed.");
    } finally {
        fs.rmSync(temp, { recursive: true, force: true });
    }
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
