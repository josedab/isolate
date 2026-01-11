.PHONY: proto build test clean

# Proto generation
proto:
	protoc --go_out=. --go_opt=paths=source_relative \
		--go-grpc_out=. --go-grpc_opt=paths=source_relative \
		--proto_path=../../proto \
		../../proto/isolate.proto

# Build
build:
	go build ./...

# Test
test:
	go test -v ./...

# Clean
clean:
	go clean ./...

# Tidy dependencies
tidy:
	go mod tidy

# Lint
lint:
	golangci-lint run ./...
