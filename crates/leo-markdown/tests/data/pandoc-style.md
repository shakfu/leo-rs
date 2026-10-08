# Pandoc Style Example

This example uses the original Pandoc/entangled code fence syntax.

To use this style, set `style = "pandoc"` in `entangled.toml` or pass `--style pandoc`
on the command line.

## A Rust program

```text {.rust #main file=main.rs}
fn main() {
    <<print-message>>
}
```

The message:

```text {.rust #print-message}
    println!("Hello from Pandoc-style entangled!");
```

## Running

```sh
entangled tangle --style pandoc
rustc main.rs && ./main
```
