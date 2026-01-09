#!/usr/bin/env python3
"""
Generate WASM test fixtures for Isolate.

These fixtures test various edge cases:
- Infinite loops (timeout testing)
- Memory allocation (memory limit testing)
- CPU-intensive operations (fuel exhaustion)
- WASI calls (capability testing)
"""

import struct

def encode_uleb128(value):
    """Encode an unsigned integer as LEB128."""
    result = []
    while True:
        byte = value & 0x7f
        value >>= 7
        if value != 0:
            byte |= 0x80
        result.append(byte)
        if value == 0:
            break
    return bytes(result)

def encode_sleb128(value):
    """Encode a signed integer as LEB128."""
    result = []
    more = True
    while more:
        byte = value & 0x7f
        value >>= 7
        if (value == 0 and (byte & 0x40) == 0) or (value == -1 and (byte & 0x40) != 0):
            more = False
        else:
            byte |= 0x80
        result.append(byte)
    return bytes(result)

def encode_string(s):
    """Encode a string with length prefix."""
    encoded = s.encode('utf-8')
    return encode_uleb128(len(encoded)) + encoded

def encode_vector(items):
    """Encode a vector with count prefix."""
    return encode_uleb128(len(items)) + b''.join(items)

def make_section(section_id, content):
    """Create a WASM section."""
    return bytes([section_id]) + encode_uleb128(len(content)) + content

# WASM magic and version
WASM_HEADER = bytes([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])

# Value types
I32 = 0x7f
I64 = 0x7e
F32 = 0x7d
F64 = 0x7c
FUNCREF = 0x70
EXTERNREF = 0x6f

# Section IDs
SECTION_TYPE = 1
SECTION_IMPORT = 2
SECTION_FUNCTION = 3
SECTION_TABLE = 4
SECTION_MEMORY = 5
SECTION_GLOBAL = 6
SECTION_EXPORT = 7
SECTION_START = 8
SECTION_ELEMENT = 9
SECTION_CODE = 10
SECTION_DATA = 11

# Export kinds
EXPORT_FUNC = 0x00
EXPORT_TABLE = 0x01
EXPORT_MEM = 0x02
EXPORT_GLOBAL = 0x03


def create_infinite_loop_wasm():
    """
    Create a WASM module with an infinite loop using a counter.
    This tests timeout/epoch interruption.

    Uses a loop that increments forever, which the epoch system can interrupt.
    """
    # Type section
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),  # type 0: (i32) -> ()
        bytes([0x60, 0x00, 0x00]),       # type 1: () -> ()
    ]))

    # Import section: wasi proc_exit
    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00])
    ]))

    # Function section
    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x01])  # function 1 uses type 1
    ]))

    # Memory section: 1 page minimum
    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])  # limits: no max, min=1
    ]))

    # Export section: memory and _start
    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x01]),
    ]))

    # Code section: infinite loop with counter
    # local $i i64
    # loop: $i = $i + 1; br to loop
    code_body = bytes([
        0x01,            # 1 local declaration group
        0x01, I64,       # 1 local of type i64
        0x42, 0x00,      # i64.const 0
        0x21, 0x00,      # local.set 0
        0x03, 0x40,      # loop (void)
        0x20, 0x00,      # local.get 0
        0x42, 0x01,      # i64.const 1
        0x7c,            # i64.add
        0x21, 0x00,      # local.set 0
        0x0c, 0x00,      # br 0 (to loop)
        0x0b,            # end loop
        0x0b,            # end function
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def create_memory_grow_wasm():
    """
    Create a WASM module that grows memory repeatedly.
    This tests memory limits.
    """
    # Type section
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),  # type 0: (i32) -> ()
        bytes([0x60, 0x00, 0x00]),       # type 1: () -> ()
    ]))

    # Import section: wasi proc_exit
    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00])
    ]))

    # Function section
    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x01])  # function 1 uses type 1
    ]))

    # Memory section
    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])  # min=1, no max
    ]))

    # Export section
    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x01]),
    ]))

    # Code section
    # Loop: try to grow memory by 10 pages, repeat 100 times, then exit
    code_body = bytes([
        0x01,            # 1 local declaration group
        0x01, I32,       # 1 local of type i32
        0x41, 0x00,      # i32.const 0
        0x21, 0x00,      # local.set 0
        0x03, 0x40,      # loop (void)
        0x41, 0x0a,      # i32.const 10
        0x40, 0x00,      # memory.grow mem=0
        0x1a,            # drop
        0x20, 0x00,      # local.get 0
        0x41, 0x01,      # i32.const 1
        0x6a,            # i32.add
        0x21, 0x00,      # local.set 0
        0x20, 0x00,      # local.get 0
        0x41, 0x64,      # i32.const 100
        0x49,            # i32.lt_u
        0x0d, 0x00,      # br_if 0
        0x0b,            # end loop
        0x41, 0x00,      # i32.const 0
        0x10, 0x00,      # call $exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def create_cpu_intensive_wasm():
    """
    Create a WASM module that burns CPU/fuel.
    Counts up in a tight loop to exhaust fuel.
    """
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),  # (i32) -> ()
        bytes([0x60, 0x00, 0x00]),       # () -> ()
    ]))

    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00])
    ]))

    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x01])
    ]))

    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])
    ]))

    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x01]),
    ]))

    # Tight loop counting to 1 billion
    code_body = bytes([
        0x01,            # 1 local declaration group
        0x01, I64,       # 1 local of type i64
        0x42, 0x00,      # i64.const 0
        0x21, 0x00,      # local.set 0
        0x03, 0x40,      # loop (void)
        0x20, 0x00,      # local.get 0
        0x42, 0x01,      # i64.const 1
        0x7c,            # i64.add
        0x21, 0x00,      # local.set 0
        0x20, 0x00,      # local.get 0
        # i64.const 1000000000 (0x3B9ACA00) - 1 billion
        0x42, 0x80, 0x94, 0xeb, 0xdc, 0x03,
        0x53,            # i64.lt_u
        0x0d, 0x00,      # br_if 0
        0x0b,            # end loop
        0x41, 0x00,      # i32.const 0
        0x10, 0x00,      # call $exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def create_stdout_write_wasm():
    """
    Create a WASM module that writes a lot to stdout.
    Tests I/O limits.
    """
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),           # type 0: (i32) -> ()
        bytes([0x60, 0x04, I32, I32, I32, I32, 0x01, I32]),  # type 1: fd_write (i32,i32,i32,i32)->i32
        bytes([0x60, 0x00, 0x00]),                # type 2: () -> ()
    ]))

    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00]),
        encode_string("wasi_snapshot_preview1") + encode_string("fd_write") + bytes([0x00, 0x01]),
    ]))

    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x02])  # function uses type 2
    ]))

    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])
    ]))

    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x02]),
    ]))

    # Data section: iov structure and string
    # At offset 0: iov_base (ptr to string) = 16
    # At offset 4: iov_len = 1
    # At offset 8: nwritten (result)
    # At offset 16: "X"
    data_content = (
        struct.pack('<I', 16) +  # iov_base = 16
        struct.pack('<I', 1) +   # iov_len = 1
        struct.pack('<I', 0) +   # nwritten placeholder
        b'\x00\x00\x00\x00' +    # padding to offset 16
        b'X'                     # the character to write
    )
    data_section = make_section(SECTION_DATA, encode_vector([
        bytes([0x00]) +          # active segment, memory 0
        bytes([0x41, 0x00, 0x0b]) +  # i32.const 0, end
        encode_uleb128(len(data_content)) + data_content
    ]))

    # Code: loop 10000 times writing to stdout (fd=1)
    code_body = bytes([
        0x01,            # 1 local declaration group
        0x01, I32,       # 1 local of type i32
        0x41, 0x00,      # i32.const 0
        0x21, 0x00,      # local.set 0
        0x03, 0x40,      # loop
        # fd_write(1, iov=0, iovs_len=1, nwritten=8)
        0x41, 0x01,      # i32.const 1 (stdout)
        0x41, 0x00,      # i32.const 0 (iov ptr)
        0x41, 0x01,      # i32.const 1 (iovs_len)
        0x41, 0x08,      # i32.const 8 (nwritten ptr)
        0x10, 0x01,      # call fd_write
        0x1a,            # drop result
        0x20, 0x00,      # local.get 0
        0x41, 0x01,      # i32.const 1
        0x6a,            # i32.add
        0x21, 0x00,      # local.set 0
        0x20, 0x00,      # local.get 0
        0x41, 0x90, 0x4e,# i32.const 10000
        0x49,            # i32.lt_u
        0x0d, 0x00,      # br_if 0
        0x0b,            # end loop
        0x41, 0x00,      # i32.const 0
        0x10, 0x00,      # call proc_exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return (WASM_HEADER + type_section + import_section + func_section +
            mem_section + export_section + code_section + data_section)


def create_env_reader_wasm():
    """
    Create a WASM module that reads environment variables.
    Tests environment capability.

    Calls environ_sizes_get and exits with the count.
    """
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),           # type 0: (i32) -> ()
        bytes([0x60, 0x02, I32, I32, 0x01, I32]), # type 1: environ_sizes_get
        bytes([0x60, 0x00, 0x00]),                # type 2: () -> ()
    ]))

    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00]),
        encode_string("wasi_snapshot_preview1") + encode_string("environ_sizes_get") + bytes([0x00, 0x01]),
    ]))

    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x02])
    ]))

    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])
    ]))

    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x02]),
    ]))

    # Memory layout:
    # 0: environ_count
    # 4: environ_buf_size
    code_body = bytes([
        0x00,            # 0 local declaration groups
        0x41, 0x00,      # i32.const 0 (count ptr)
        0x41, 0x04,      # i32.const 4 (buf_size ptr)
        0x10, 0x01,      # call environ_sizes_get
        0x1a,            # drop result
        # Exit with environ_count
        0x41, 0x00,      # i32.const 0
        0x28, 0x02, 0x00,# i32.load offset=0
        0x10, 0x00,      # call proc_exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def create_args_reader_wasm():
    """
    Create a WASM module that reads command-line arguments.
    Tests args capability.

    Calls args_sizes_get and exits with the count.
    """
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),           # type 0: (i32) -> ()
        bytes([0x60, 0x02, I32, I32, 0x01, I32]), # type 1: args_sizes_get
        bytes([0x60, 0x00, 0x00]),                # type 2: () -> ()
    ]))

    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00]),
        encode_string("wasi_snapshot_preview1") + encode_string("args_sizes_get") + bytes([0x00, 0x01]),
    ]))

    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x02])
    ]))

    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])
    ]))

    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x02]),
    ]))

    code_body = bytes([
        0x00,            # 0 local declaration groups
        0x41, 0x00,      # i32.const 0 (argc ptr)
        0x41, 0x04,      # i32.const 4 (argv_buf_size ptr)
        0x10, 0x01,      # call args_sizes_get
        0x1a,            # drop result
        # Exit with argc
        0x41, 0x00,      # i32.const 0
        0x28, 0x02, 0x00,# i32.load offset=0
        0x10, 0x00,      # call proc_exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def create_clock_reader_wasm():
    """
    Create a WASM module that reads the clock.
    Tests time capability.

    Calls clock_time_get and exits with result code.
    """
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),              # type 0: (i32) -> ()
        bytes([0x60, 0x03, I32, I64, I32, 0x01, I32]),  # type 1: clock_time_get
        bytes([0x60, 0x00, 0x00]),                   # type 2: () -> ()
    ]))

    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00]),
        encode_string("wasi_snapshot_preview1") + encode_string("clock_time_get") + bytes([0x00, 0x01]),
    ]))

    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x02])
    ]))

    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])
    ]))

    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x02]),
    ]))

    # clock_time_get(clock_id=0 realtime, precision=1000, timestamp_ptr=0)
    code_body = bytes([
        0x00,            # 0 local declaration groups
        0x41, 0x00,      # i32.const 0 (realtime clock)
        0x42, 0xe8, 0x07,# i64.const 1000 (precision)
        0x41, 0x00,      # i32.const 0 (timestamp ptr)
        0x10, 0x01,      # call clock_time_get
        # Exit with result (0 = success)
        0x10, 0x00,      # call proc_exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def create_random_reader_wasm():
    """
    Create a WASM module that reads random bytes.
    Tests random capability.

    Calls random_get and exits with result code.
    """
    type_section = make_section(SECTION_TYPE, encode_vector([
        bytes([0x60, 0x01, I32, 0x00]),           # type 0: (i32) -> ()
        bytes([0x60, 0x02, I32, I32, 0x01, I32]), # type 1: random_get
        bytes([0x60, 0x00, 0x00]),                # type 2: () -> ()
    ]))

    import_section = make_section(SECTION_IMPORT, encode_vector([
        encode_string("wasi_snapshot_preview1") + encode_string("proc_exit") + bytes([0x00, 0x00]),
        encode_string("wasi_snapshot_preview1") + encode_string("random_get") + bytes([0x00, 0x01]),
    ]))

    func_section = make_section(SECTION_FUNCTION, encode_vector([
        bytes([0x02])
    ]))

    mem_section = make_section(SECTION_MEMORY, encode_vector([
        bytes([0x00, 0x01])
    ]))

    export_section = make_section(SECTION_EXPORT, encode_vector([
        encode_string("memory") + bytes([EXPORT_MEM, 0x00]),
        encode_string("_start") + bytes([EXPORT_FUNC, 0x02]),
    ]))

    # random_get(buf=0, buf_len=8)
    code_body = bytes([
        0x00,            # 0 local declaration groups
        0x41, 0x00,      # i32.const 0 (buf ptr)
        0x41, 0x08,      # i32.const 8 (8 bytes)
        0x10, 0x01,      # call random_get
        # Exit with result (0 = success)
        0x10, 0x00,      # call proc_exit
        0x0b,            # end
    ])
    code_section = make_section(SECTION_CODE, encode_vector([
        encode_uleb128(len(code_body)) + code_body
    ]))

    return WASM_HEADER + type_section + import_section + func_section + mem_section + export_section + code_section


def main():
    import os

    fixtures = [
        ("infinite_loop.wasm", create_infinite_loop_wasm()),
        ("memory_grow.wasm", create_memory_grow_wasm()),
        ("cpu_intensive.wasm", create_cpu_intensive_wasm()),
        ("stdout_flood.wasm", create_stdout_write_wasm()),
        ("env_reader.wasm", create_env_reader_wasm()),
        ("args_reader.wasm", create_args_reader_wasm()),
        ("clock_reader.wasm", create_clock_reader_wasm()),
        ("random_reader.wasm", create_random_reader_wasm()),
    ]

    script_dir = os.path.dirname(os.path.abspath(__file__))

    for name, wasm in fixtures:
        path = os.path.join(script_dir, name)
        with open(path, 'wb') as f:
            f.write(wasm)
        print(f"Generated {name} ({len(wasm)} bytes)")


if __name__ == "__main__":
    main()
