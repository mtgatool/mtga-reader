# mtga-reader

![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/mtgatool/mtga-reader/CI.yml) ![GitHub Release](https://img.shields.io/github/v/release/mtgatool/mtga-reader) ![GitHub License](https://img.shields.io/github/license/mtgatool/mtga-reader) ![npm](https://img.shields.io/npm/v/mtga-reader)

A high-performance native library for reading Magic: The Gathering Arena game memory. Extract player data, card collections, inventory, and more directly from the running game process.

## Features

- **Cross-platform** - Windows and Linux support with prebuilt binaries
- **Node.js bindings** - First-class npm package powered by [napi-rs](https://napi.rs)
- **Memory introspection** - Browse assemblies, classes, instances, and dictionaries
- **Zero runtime dependencies** - Pure Rust core with no external requirements

## Installation

```bash
npm install mtga-reader
# or
yarn add mtga-reader
```

## Quick Start

```javascript
const mtga = require('mtga-reader');

// Check if MTGA is running and read card collection
if (mtga.findPidByName('MTGA')) {
    const cards = mtga.readData('MTGA', [
        'WrapperController',
        '<Instance>k__BackingField',
        '<InventoryManager>k__BackingField',
        '_inventoryServiceWrapper',
        '<Cards>k__BackingField',
        '_entries',
    ]);
    console.log(`You have ${cards.length} unique cards!`);
}
```

> **Note**: Reading game memory requires administrator/root privileges.

## API

### Core Functions

| Function | Description |
|----------|-------------|
| `readData(processName, path)` | Traverse a path of fields from a root class |
| `readClass(processName, address)` | Read a managed class at a memory address |
| `readGenericInstance(processName, address)` | Read a generic instance at an address |
| `findPidByName(processName)` | Check if a process exists |
| `isAdmin()` | Check for administrator privileges |

## Test Projects

This repository includes two test projects for development and debugging:

### debug-ui

A React-based web interface for exploring MTGA memory structures interactively.

```bash
# Start the HTTP server (requires admin privileges)
cargo run --bin http_server_simple

# In another terminal, start the UI
cd debug-ui
npm install && npm run dev
```

Browse assemblies, inspect classes, read static instances, and explore dictionaries through a visual interface at `http://localhost:3000`.

### node-test

A standalone Node.js test project demonstrating the native addon API.

```bash
cd node-test
npm install
npm run build
npm test  # Run as administrator
```

Tests the full API including assembly enumeration, class inspection, and data reading paths.

## Development

### Prerequisites

- Rust toolchain (stable)
- Node.js 16+
- MTGA installed and running (for testing)

### Building

```bash
# Development build
yarn build:debug

# Production build
yarn build
```

### Testing

```bash
# Run with MTGA open (requires admin)
cargo run --bin debug
```

Tests in `lib.rs` require MTGA to be running and cannot be automated in CI.

## Releases

All releases are automated via GitHub Actions:

```bash
npm version patch|minor|major
git push --follow-tags
```

This triggers the napi-rs workflow which builds binaries for all platforms and publishes to npm.

---

### About

This is a Rust port of [Unity Spy](https://github.com/hackf5/unityspy), adapted for MTGA. The codebase reads Mono runtime structures directly from memory, enabling access to game state without log file parsing.

**References:**
- [Mono source (Unity fork)](https://github.com/Unity-Technologies/mono/blob/2021.3.14f1/mono/metadata/domain-internals.h)
- [Original article on Unity memory reading](https://hackf5.medium.com/hacking-into-unity-games-ca99f87954c)
