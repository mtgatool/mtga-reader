/**
 * Copy the compiled Rust library to a .node file
 */
const fs = require('fs');
const path = require('path');

const isDebug = process.argv[2] === 'debug';
const buildType = isDebug ? 'debug' : 'release';

// Source: Rust outputs a .dll on Windows
const sourcePath = path.join(__dirname, '..', 'target', buildType, 'mtga_reader_node.dll');

// Destination: Node.js expects a .node file
const destPath = path.join(__dirname, '..', 'mtga_reader_node.node');

if (!fs.existsSync(sourcePath)) {
    console.error(`Error: Build output not found at ${sourcePath}`);
    console.error('Make sure cargo build completed successfully.');
    process.exit(1);
}

// Copy and rename
fs.copyFileSync(sourcePath, destPath);
console.log(`Copied ${sourcePath} -> ${destPath}`);
console.log('Build complete! You can now run: npm test');
