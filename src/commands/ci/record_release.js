// semantic-release plugin for the dry-run pass: writes each package's next
// version and release notes to `<dir>/<name>.json`
const fs = require("fs");
const path = require("path");

module.exports = {
  generateNotes({ dir, name }, { nextRelease }) {
    const notes = (nextRelease.notes || "").replace(
      /^(#+) (\[?\d+\.\d+\.\d+\]?)/,
      `$1 ${name} $2`
    );
    fs.writeFileSync(
      path.join(dir, `${name}.json`),
      JSON.stringify({ version: nextRelease.version, notes })
    );
  },
};
