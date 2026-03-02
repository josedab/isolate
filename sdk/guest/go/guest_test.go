package isolate

import (
	"bytes"
	"encoding/json"
	"io"
	"os"
	"strings"
	"testing"
)

// ---------------------------------------------------------------------------
// GuestError
// ---------------------------------------------------------------------------

func TestGuestError(t *testing.T) {
	err := NewGuestError("something failed")
	if err.Error() != "guest error: something failed" {
		t.Errorf("unexpected error message: %s", err.Error())
	}
}

// ---------------------------------------------------------------------------
// ReadInput / ReadRaw
// ---------------------------------------------------------------------------

func TestReadInputValid(t *testing.T) {
	type Input struct {
		Name  string `json:"name"`
		Value int    `json:"value"`
	}

	data := `{"name":"test","value":42}`
	old := os.Stdin
	r, w, _ := os.Pipe()
	w.Write([]byte(data))
	w.Close()
	os.Stdin = r
	defer func() { os.Stdin = old }()

	input, err := ReadInput[Input]()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if input.Name != "test" || input.Value != 42 {
		t.Errorf("unexpected input: %+v", input)
	}
}

func TestReadInputEmpty(t *testing.T) {
	type Input struct {
		Name string `json:"name"`
	}

	old := os.Stdin
	r, w, _ := os.Pipe()
	w.Close()
	os.Stdin = r
	defer func() { os.Stdin = old }()

	input, err := ReadInput[Input]()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if input.Name != "" {
		t.Errorf("expected zero value, got: %+v", input)
	}
}

func TestReadRaw(t *testing.T) {
	payload := []byte{0x00, 0x01, 0x02, 0xFF}
	old := os.Stdin
	r, w, _ := os.Pipe()
	w.Write(payload)
	w.Close()
	os.Stdin = r
	defer func() { os.Stdin = old }()

	data, err := ReadRaw()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !bytes.Equal(data, payload) {
		t.Errorf("expected %v, got %v", payload, data)
	}
}

// ---------------------------------------------------------------------------
// WriteOutput / WriteRaw
// ---------------------------------------------------------------------------

func TestWriteOutput(t *testing.T) {
	type Output struct {
		Result string `json:"result"`
	}

	old := os.Stdout
	r, w, _ := os.Pipe()
	os.Stdout = w

	err := WriteOutput(Output{Result: "ok"})
	w.Close()
	os.Stdout = old

	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	buf, _ := io.ReadAll(r)
	var parsed Output
	if err := json.Unmarshal(bytes.TrimSpace(buf), &parsed); err != nil {
		t.Fatalf("failed to parse output: %v", err)
	}
	if parsed.Result != "ok" {
		t.Errorf("unexpected output: %+v", parsed)
	}
}

func TestWriteRaw(t *testing.T) {
	payload := []byte("hello raw")
	old := os.Stdout
	r, w, _ := os.Pipe()
	os.Stdout = w

	err := WriteRaw(payload)
	w.Close()
	os.Stdout = old

	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	buf, _ := io.ReadAll(r)
	if !bytes.Equal(buf, payload) {
		t.Errorf("expected %q, got %q", payload, buf)
	}
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

func TestGetEnv(t *testing.T) {
	os.Setenv("ISOLATE_TEST_VAR", "hello")
	defer os.Unsetenv("ISOLATE_TEST_VAR")

	val := GetEnv("ISOLATE_TEST_VAR")
	if val != "hello" {
		t.Errorf("expected 'hello', got %q", val)
	}
}

func TestGetEnvMissing(t *testing.T) {
	val := GetEnv("DEFINITELY_NOT_SET_12345")
	if val != "" {
		t.Errorf("expected empty string, got %q", val)
	}
}

func TestGetAllEnv(t *testing.T) {
	os.Setenv("ISOLATE_TEST_A", "1")
	os.Setenv("ISOLATE_TEST_B", "2")
	defer os.Unsetenv("ISOLATE_TEST_A")
	defer os.Unsetenv("ISOLATE_TEST_B")

	env := GetAllEnv()
	if env["ISOLATE_TEST_A"] != "1" || env["ISOLATE_TEST_B"] != "2" {
		t.Errorf("unexpected env: %v", env)
	}
}

func TestGetArgs(t *testing.T) {
	args := GetArgs()
	if len(args) == 0 {
		t.Error("expected at least one arg (the test binary)")
	}
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

func TestLogFunctions(t *testing.T) {
	tests := []struct {
		name   string
		fn     func(string)
		prefix string
	}{
		{"debug", LogDebug, "[DEBUG]"},
		{"info", LogInfo, "[INFO]"},
		{"warn", LogWarn, "[WARN]"},
		{"error", LogError, "[ERROR]"},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			old := os.Stderr
			r, w, _ := os.Pipe()
			os.Stderr = w

			tc.fn("test message")

			w.Close()
			os.Stderr = old

			buf, _ := io.ReadAll(r)
			output := string(buf)
			if !strings.Contains(output, tc.prefix) {
				t.Errorf("expected %q in output, got: %s", tc.prefix, output)
			}
			if !strings.Contains(output, "test message") {
				t.Errorf("expected 'test message' in output, got: %s", output)
			}
		})
	}
}
