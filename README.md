# mtga-reader

WIP

A Rust port of [Unity Spy](https://github.com/hackf5/unityspy). The codebase can be asily adapted to work with any Unity game, provided the Unity version matches or you re-adapt the offsets.


The original C# code is a little restrictive to build and distribute, hopefully this can enable a more portable codebase; being able to directly include precompiled binaries into any kind of projects (like Electron/NodeJs apps), without having to worry about OS compatibility and intermediate softwares.

Mono definitions reference;
https://github.com/Unity-Technologies/mono/blob/2021.3.14f1/mono/metadata/domain-internals.h


Original article explaining the basics still applies;
https://hackf5.medium.com/hacking-into-unity-games-ca99f87954c