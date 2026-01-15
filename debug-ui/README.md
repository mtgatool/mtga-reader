# MTGA Reader Debug UI

A React-based web interface for browsing and exploring MTGA game memory structures.

## Features

- **Assembly Browser**: Browse all loaded .NET assemblies in the MTGA process
- **Class Explorer**: View classes, their fields, and static instances
  - Visual indicators for pointer types (→)
  - Click "Read Static Value" to read static field pointer values
  - Click on field values to navigate to instances
- **Instance Viewer**: Inspect object instances with a tree view of field values
  - Click 📖 button to read instance field values (e.g., Gems, Gold)
  - Automatic detection and reading of Dictionary types
  - Click "Read Dictionary" to browse Dictionary contents
  - Display up to 1000 dictionary entries
- **Navigation**: Click on pointer values to navigate to referenced objects

## Getting Started

### Prerequisites

- Node.js 16+ installed
- Rust toolchain (for building the HTTP server)
- MTGA running
- Administrator privileges (required to read game memory)

### Setup

#### 1. Build and Start the HTTP Server

The HTTP server reads MTGA's memory and provides the REST API for the UI:

```bash
# From the mtga-reader root directory
cargo build --bin http_server_simple

# Run with administrator privileges (required for memory reading)
# On Windows (in PowerShell as Admin):
.\target\debug\http_server_simple.exe

# On Linux (with sudo):
sudo ./target/debug/http_server_simple
```

The server will:
- Automatically find the MTGA process
- Connect to the Mono runtime
- Start listening on `http://localhost:8080`

**Note**: MTGA must be running before you start the server.

#### 2. Install and Run the Debug UI

```bash
cd debug-ui
npm install
npm run dev
```

The UI will be available at `http://localhost:3000`

### Building for Production

```bash
npm run build
```

The built files will be in the `dist` directory.

## HTTP Server API

The UI expects a REST API running on `http://localhost:8080` with the following endpoints:

### `GET /assemblies`
Returns list of loaded assemblies.

Response:
```json
{
  "assemblies": ["Assembly-CSharp", "Core", ...]
}
```

### `GET /assembly/:name/classes`
Returns all classes in the specified assembly.

Response:
```json
{
  "classes": [
    {
      "name": "ClassName",
      "namespace": "Namespace.Name",
      "address": 123456789,
      "is_static": false,
      "is_enum": false
    }
  ]
}
```

### `GET /assembly/:assembly/class/:className`
Returns class definition with fields and static instances.

Response:
```json
{
  "name": "ClassName",
  "namespace": "Namespace.Name",
  "address": 123456789,
  "fields": [
    {
      "name": "fieldName",
      "type": "int",
      "offset": 16,
      "is_static": false,
      "is_const": false
    }
  ],
  "static_instances": [
    {
      "field_name": "_instance",
      "address": 987654321
    }
  ]
}
```

### `GET /instance/:address`
Returns instance data with field values.

Response:
```json
{
  "class_name": "ClassName",
  "namespace": "Namespace.Name",
  "address": 123456789,
  "fields": [
    {
      "name": "fieldName",
      "type": "int",
      "is_static": false,
      "value": 42
    },
    {
      "name": "refField",
      "type": "OtherClass",
      "is_static": false,
      "value": {
        "type": "pointer",
        "address": 111222333,
        "class_name": "OtherClass"
      }
    }
  ]
}
```

### `GET /instance/:address/field/:fieldName`
Reads a specific field value from an instance (useful for reading int/uint fields like Gems/Gold).

Response:
```json
{
  "type": "primitive",
  "value_type": "int32",
  "value": 1750
}
```

### `GET /class/:address/field/:fieldName`
Reads a static field value from a class.

Response:
```json
{
  "type": "pointer",
  "address": 123456789,
  "field_name": "_instance",
  "class_name": "ClassName"
}
```

### `GET /dictionary/:address`
Reads entries from a Dictionary<K,V> object.

Response:
```json
{
  "count": 250,
  "entries": [
    {
      "key": 68000,
      "value": 4
    },
    {
      "key": 68001,
      "value": 2
    }
  ]
}
```

## Technology Stack

- **React 18**: UI framework
- **Vite**: Build tool and dev server
- **CSS3**: Styling (VS Code Dark+ theme inspired)

## UI Design

The interface follows a three-column layout:

1. **Left Panel**: Assembly browser with search
2. **Middle Panel**: Class explorer (split into class list and class details)
3. **Right Panel**: Instance viewer with expandable tree structure

The color scheme is inspired by VS Code's Dark+ theme for familiarity.
