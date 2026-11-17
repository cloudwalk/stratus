"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

function isVersionAtLeast(version, minimum) {
    const versionParts = version.split(".").map(Number);
    const minimumParts = minimum.split(".").map(Number);
    if (versionParts.length !== minimumParts.length || versionParts.some((part) => !Number.isInteger(part))) {
        return false;
    }
    for (let index = 0; index < minimumParts.length; index += 1) {
        if (versionParts[index] !== minimumParts[index]) {
            return versionParts[index] > minimumParts[index];
        }
    }
    return true;
}

async function main() {
    const root = process.cwd();
    const lodashLockfiles = [
        path.resolve(__dirname, "..", "package-lock.json"),
        path.resolve(__dirname, "..", "cloudwalk-contracts", "integration", "package-lock.json"),
    ];
    for (const lockfile of lodashLockfiles) {
        const lock = JSON.parse(fs.readFileSync(lockfile, "utf8"));
        const lodashPackages = Object.entries(lock.packages ?? {}).filter(
            ([packagePath]) => packagePath === "node_modules/lodash" || packagePath.endsWith("/node_modules/lodash"),
        );
        assert.ok(lodashPackages.length > 0, `${lockfile} does not lock lodash`);
        for (const [packagePath, packageInfo] of lodashPackages) {
            const lodashVersion = packageInfo.version;
            assert.ok(lodashVersion, `${lockfile} does not record a version for ${packagePath}`);
            assert.ok(
                isVersionAtLeast(lodashVersion, "4.18.1"),
                `${lockfile} locks vulnerable lodash ${lodashVersion} at ${packagePath}`,
            );
        }
    }

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

        console.log("Lodash lockfiles, Mocha, sc-istanbul, globby, and shelljs compatibility passed.");
    } finally {
        fs.rmSync(temp, { recursive: true, force: true });
    }
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
