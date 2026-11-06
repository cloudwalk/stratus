"use strict";

const fs = require("node:fs");
const path = require("node:path");

const target = path.join(process.cwd(), "node_modules/sc-istanbul/lib/util/file-matcher.js");
const source = fs.readFileSync(target, "utf8");
const oldBlock = `    glob(includes.join(' '), opts, function (err, files) {
        if (err) { return callback(err); }
        if (relative) { return callback(err, files); }

        if (!realpath) {
            files = files.map(function (file) { return path.resolve(root, file); });
            return callback(err, files);
        }

        var realPathCache = module.constructor._realpathCache || {};

        async.map(files, function (file, done) {
            fs.realpath(path.resolve(root, file), realPathCache, done);
        }, callback);
    });`;
const newBlock = `    glob.glob(includes.join(' '), opts).then(function (files) {
        if (relative) { return callback(null, files); }

        if (!realpath) {
            files = files.map(function (file) { return path.resolve(root, file); });
            return callback(null, files);
        }

        var realPathCache = module.constructor._realpathCache || {};

        async.map(files, function (file, done) {
            fs.realpath(path.resolve(root, file), realPathCache, done);
        }, callback);
    }, callback);`;

if (source.includes(newBlock)) {
    process.exit(0);
}
if (!source.includes(oldBlock)) {
    throw new Error(`Unexpected sc-istanbul file-matcher.js contents at ${target}`);
}
fs.writeFileSync(target, source.replace(oldBlock, newBlock));
