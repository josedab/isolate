package isolate_test

import (
	"context"
	"fmt"
	"log"

	isolate "github.com/josedab/isolate/sdk/go"
)

func Example_basic() {
	// Create a client
	client, err := isolate.NewClient("localhost:50051")
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	// Minimal WASM module that exits with code 0
	wasmBytes := []byte{
		0x00, 0x61, 0x73, 0x6d, // Magic number
		0x01, 0x00, 0x00, 0x00, // Version
	}

	// Execute the WASM module
	ctx := context.Background()
	result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
		MemoryLimit: 64 * 1024 * 1024,
		Capabilities: []isolate.Capability{
			isolate.Stdout(),
			isolate.Stderr(),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Exit code: %d\n", result.ExitCode)
}

func Example_withCapabilities() {
	client, err := isolate.NewClient("localhost:50051")
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	wasmBytes := []byte{ /* your WASM module */ }

	ctx := context.Background()
	result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
		MemoryLimit: 128 * 1024 * 1024,
		FuelLimit:   50_000_000,
		Capabilities: []isolate.Capability{
			isolate.Stdout(),
			isolate.Stderr(),
			isolate.FsRead("/data"),
			isolate.HTTP("*.example.com"),
			isolate.Env("API_KEY"),
		},
		Env: map[string]string{
			"API_KEY": "secret-value",
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Stdout: %s\n", string(result.Stdout))
}

func Example_separateCreateAndRun() {
	client, err := isolate.NewClient("localhost:50051")
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	wasmBytes := []byte{ /* your WASM module */ }
	ctx := context.Background()

	// Create sandbox
	createResult, err := client.CreateSandbox(ctx, wasmBytes, &isolate.CreateSandboxOptions{
		MemoryLimit: 64 * 1024 * 1024,
		Capabilities: []isolate.Capability{
			isolate.Stdout(),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Sandbox ID: %s\n", createResult.SandboxID)

	// Run sandbox multiple times
	for i := 0; i < 3; i++ {
		result, err := client.RunSandbox(ctx, createResult.SandboxID, &isolate.RunSandboxOptions{
			Input: []byte(fmt.Sprintf("iteration %d", i)),
		})
		if err != nil {
			log.Fatal(err)
		}
		fmt.Printf("Run %d exit code: %d\n", i, result.ExitCode)
	}

	// Terminate sandbox
	_, err = client.TerminateSandbox(ctx, createResult.SandboxID)
	if err != nil {
		log.Fatal(err)
	}
}

func Example_listSandboxes() {
	client, err := isolate.NewClient("localhost:50051")
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	ctx := context.Background()

	// List all sandboxes in "ready" state
	result, err := client.ListSandboxes(ctx, &isolate.ListSandboxesOptions{
		StateFilter: "ready",
		Limit:       10,
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Total sandboxes: %d\n", result.Total)
	for _, sandbox := range result.Sandboxes {
		fmt.Printf("  - %s: %s\n", sandbox.ID, sandbox.State)
	}
}
