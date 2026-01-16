/**
 * Script to copy the compiled native module to the package directory
 */

const fs = require('fs');
const path = require('path');

const isDebug = process.argv[2] === 'debug';
const buildType = isDebug ? 'debug' : 'release';

const rootDir = path.join(__dirname, '..', '..');
const targetDir = path.join(rootDir, 'target', buildType);
const outputDir = path.join(__dirname, '..');

// Platform-specific library names
const libraryNames = {
  darwin: 'libmtga_reader.dylib',
  win32: 'mtga_reader.dll',
  linux: 'libmtga_reader.so',
};

const platform = process.platform;
const arch = process.arch;

const sourceLib = libraryNames[platform];
if (!sourceLib) {
  console.error(`Unsupported platform: ${platform}`);
  process.exit(1);
}

const sourcePath = path.join(targetDir, sourceLib);
const targetName = 'mtga_reader.node';
const targetPath = path.join(outputDir, targetName);

console.log(`Copying ${sourcePath} -> ${targetPath}`);

if (!fs.existsSync(sourcePath)) {
  console.error(`Source file not found: ${sourcePath}`);
  console.error(`Did you run: cargo build --${buildType} --lib --features napi-bindings?`);
  process.exit(1);
}

fs.copyFileSync(sourcePath, targetPath);
console.log(`Successfully copied native module to ${targetPath}`);

// Also create platform-specific name for better identification
const platformTargetName = `mtga_reader.${platform}-${arch}.node`;
const platformTargetPath = path.join(outputDir, platformTargetName);
fs.copyFileSync(sourcePath, platformTargetPath);
console.log(`Also created: ${platformTargetPath}`);
