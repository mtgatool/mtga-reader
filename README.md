# mtga-reader

![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/mtgatool/mtga-reader/CI.yml) ![GitHub Release](https://img.shields.io/github/v/release/mtgatool/mtga-reader) ![GitHub License](https://img.shields.io/github/license/mtgatool/mtga-reader)


WIP

A Rust port of [Unity Spy](https://github.com/hackf5/unityspy). The codebase can be asily adapted to work with any Unity game, provided the Unity version matches or you re-adapt the offsets.


The original C# code is a little restrictive to build and distribute, hopefully this can enable a more portable codebase; being able to directly include precompiled binaries into any kind of projects (like Electron/NodeJs apps), without having to worry about OS compatibility and intermediate softwares.

Mono definitions reference;
https://github.com/Unity-Technologies/mono/blob/2021.3.14f1/mono/metadata/domain-internals.h


Original article explaining the basics still applies;
https://hackf5.medium.com/hacking-into-unity-games-ca99f87954c


## Development

You can use `cargo run --bin debug` to run a small script in `/src/bin` that should serve as a quick development/check for your changes. However, there are tests in `lib.rs`. and those should always be passing before building. These tests depend on MTG Arena to be running in order to pass, therefore we cant currently run these on CI for automation. For any important path we discover/fix, a new test covering it should be added.

## Building

All builds should go automatically with `npm version && git push --follow-tags`, that triggers the `napi-rs` workflow and releases to npm automatically.

For local builds you can simply run `yarn build`.