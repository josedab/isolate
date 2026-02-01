# Isolate Guest SDK — Go

Write WASM modules for Isolate sandboxes in Go.

## Quick Start

### Prerequisites

- Go 1.21+ with `wasip1` support:

```bash
go version  # must be 1.21 or later
```

### Project Setup

```bash
mkdir my-guest-module && cd my-guest-module
go mod init my-guest-module
```

Copy `guest.go` from this directory into your project, or import it as a module.

### Writing a Guest Module

```go
package main

import (
    isolate "my-guest-module/isolate"
)

type Input struct {
    Name string `json:"name"`
}

type Output struct {
    Greeting string `json:"greeting"`
}

func main() {
    input, err := isolate.ReadInput[Input]()
    if err != nil {
        isolate.LogError("failed to read input: " + err.Error())
        os.Exit(1)
    }

    output := Output{
        Greeting: fmt.Sprintf("Hello, %s!", input.Name),
    }

    if err := isolate.WriteOutput(output); err != nil {
        isolate.LogError("failed to write output: " + err.Error())
        os.Exit(1)
    }
}
```

### Building

```bash
GOOS=wasip1 GOARCH=wasm go build -o my_module.wasm .
```

### Running

```bash
echo '{"name": "World"}' | isolate run \
    --capability stdout \
    --capability stdin \
    my_module.wasm
# Output: {"greeting":"Hello, World!"}
```

## API Reference

### `ReadInput[T any]() (T, error)`

Reads and unmarshals JSON input from stdin into the specified type.

### `WriteOutput[T any](output T) error`

Marshals the value as JSON and writes it to stdout.

### `GetEnv(name string) string`

Returns the value of an environment variable (requires env capability).

### `LogInfo(msg string)`, `LogWarn(msg string)`, `LogError(msg string)`

Write structured log messages to stderr.

## Notes

- Go's WASM output is larger than Rust/C. Consider using TinyGo for smaller binaries:
  ```bash
  tinygo build -o my_module.wasm -target=wasip1 .
  ```
- Go's `wasip1` support is stable as of Go 1.21.
