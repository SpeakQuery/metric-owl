# BUILT IN RUST

Metric Owl's monitoring agent, local API, command-line interface and optional
notification relay are written in **Rust**. We chose it for software that runs
continuously beside the workloads it observes.

## MEMORY SAFETY THROUGH OWNERSHIP

Safe Rust checks which code owns a value and which references may access it.
Ownership and borrowing rules prevent classes of invalid memory access, including
use-after-free and conflicting mutable access, before code compiles. These rules
also give resources clear lifetimes. [Rust ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html).

## NO LANGUAGE-LEVEL GARBAGE COLLECTOR

Ownership releases resources when their owners leave scope. Rust does not require
a garbage collector to periodically find unused objects. This is a useful
foundation for controlling memory and resource lifetimes in a continuously running
agent. It does not replace bounded collection, retention or workload measurement.
[Why Rust](https://rust-lang.org/).

## NATIVE CODE AND A SIMPLE RUNTIME PACKAGE

The public Linux x86-64 package contains a statically linked musl executable and
embedded frontend assets. Customers do not need a Rust compiler, Node.js VM or
Cargo build on their server to run that executable. The guided installer and HTTPS
maintenance helpers use Bash/Python; those dependencies are described in the
installation guide. Tokio supplies the agent's asynchronous runtime; “native” does
not mean the application has no libraries or operating-system dependencies.

## CONCURRENCY CHECKS BEFORE RELEASE

Rust's ownership, type system, Send and Sync traits catch many invalid patterns
of sharing data between concurrent workers. That helps us maintain metric
collection, API requests and notification work without relying solely on runtime
reproduction of a race. [Rust concurrency](https://doc.rust-lang.org/book/ch16-00-concurrency.html).

## EXPLICIT FAILURE AND MISSING-DATA STATES

Result represents recoverable failures; Option represents a value that may be
absent. These types encourage explicit handling of unsuccessful probes and
unavailable readings. Metric Owl also reports coverage and integrity separately:
missing history is never converted into a healthy zero. [Rust error handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html).

## USEFUL DIAGNOSTICS AND REPEATABLE TOOLING

Compiler diagnostics identify invalid code early. Cargo tests, rustfmt and strict
Clippy checks are part of this repository's validation workflow. Linux packaging,
browser tests and real installation checks validate the delivered product beyond
the language choice. Language features are not a claim that software has no bugs
or that a particular CPU/RAM overhead has been achieved without measurement.

The browser UI is React/TypeScript. Rust powers the backend; we describe the other
parts separately so installation assistants can choose the right tools.
