# Multi-File Example

Demonstrates cross-references between code blocks and multi-file output.

## Project structure

We build a small Python project with a main entry point and a utility module.

### Utilities module

```python #greet file=utils.py
def greet(name):
    <<greeting-body>>
```

The greeting body formats and prints a message:

```python #greeting-body
    message = f"Hello, {name}!"
    print(message)
    return message
```

### Main entry point

The main script imports and uses the utility function:

```python #main file=main.py
<<imports>>

def main():
    <<main-body>>

if __name__ == "__main__":
    main()
```

```python #imports
from utils import greet
```

```python #main-body
    names = ["Alice", "Bob", "Carol"]
    for name in names:
        greet(name)
```

## Running

```sh
entangled tangle
python main.py
```

## Notes

- `<<imports>>` and `<<main-body>>` are expanded inline during tangle
- `<<greeting-body>>` is indented to match the function body
- Each `file=...` attribute creates a separate output file
