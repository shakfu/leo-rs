# Classical Ciphers in Python

This project implements two foundational ciphers from cryptography—the
**Caesar cipher** and the **Vigen&egrave;re cipher**—and wraps them in a small
command-line tool. The goal is to show how literate programming lets us
interleave the *why* (mathematical reasoning) with the *what* (working code).

## The alphabet

Both ciphers operate on the 26-letter Latin alphabet. We define it once and
reuse it everywhere.

```python #alphabet
ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
```

## Caesar cipher

The Caesar cipher shifts every letter by a fixed number of positions.
Formally, for a letter at position $p$ in the alphabet and a shift $k$:

$$E(p) = (p + k) \mod 26$$
$$D(p) = (p - k) \mod 26$$

Non-alphabetic characters pass through unchanged.

### Encryption

```python #caesar-encrypt
def caesar_encrypt(plaintext: str, shift: int) -> str:
    """Encrypt plaintext with a Caesar cipher."""
    result = []
    for ch in plaintext.upper():
        if ch in ALPHABET:
            idx = (ALPHABET.index(ch) + shift) % 26
            result.append(ALPHABET[idx])
        else:
            result.append(ch)
    return "".join(result)
```

### Decryption

Decryption is just encryption with the negated shift:

```python #caesar-decrypt
def caesar_decrypt(ciphertext: str, shift: int) -> str:
    """Decrypt a Caesar cipher."""
    return caesar_encrypt(ciphertext, -shift)
```

## Vigen&egrave;re cipher

The Vigen&egrave;re cipher generalises Caesar by using a **keyword** instead of
a single shift. Each letter of the keyword provides the shift for the
corresponding plaintext letter, cycling through the keyword as needed.

For keyword letter $k_i$ and plaintext letter $p_i$:

$$E(p_i) = (p_i + k_i) \mod 26$$
$$D(c_i) = (c_i - k_i) \mod 26$$

### Key stream

First we need a helper that generates an infinite stream of shift values from
the keyword, skipping over non-alphabetic positions in the plaintext:

```python #key-stream
def key_stream(keyword: str):
    """Yield shift values from a keyword, cycling indefinitely."""
    keyword = keyword.upper()
    idx = 0
    while True:
        shift = ALPHABET.index(keyword[idx % len(keyword)])
        yield shift
        idx += 1
```

### Encryption

```python #vigenere-encrypt
def vigenere_encrypt(plaintext: str, keyword: str) -> str:
    """Encrypt plaintext with a Vigenere cipher."""
    ks = key_stream(keyword)
    result = []
    for ch in plaintext.upper():
        if ch in ALPHABET:
            shift = next(ks)
            idx = (ALPHABET.index(ch) + shift) % 26
            result.append(ALPHABET[idx])
        else:
            result.append(ch)
    return "".join(result)
```

### Decryption

```python #vigenere-decrypt
def vigenere_decrypt(ciphertext: str, keyword: str) -> str:
    """Decrypt a Vigenere cipher."""
    ks = key_stream(keyword)
    result = []
    for ch in ciphertext.upper():
        if ch in ALPHABET:
            shift = next(ks)
            idx = (ALPHABET.index(ch) - shift) % 26
            result.append(ALPHABET[idx])
        else:
            result.append(ch)
    return "".join(result)
```

## The cipher library

All the pieces above are collected into a single module:

```python #ciphers file=ciphers.py
"""Classical cipher implementations."""

<<alphabet>>

# --- Caesar cipher ---------------------------------------------------

<<caesar-encrypt>>

<<caesar-decrypt>>

# --- Vigenere cipher --------------------------------------------------

<<key-stream>>

<<vigenere-encrypt>>

<<vigenere-decrypt>>
```

## Command-line interface

The CLI uses only the standard library. It accepts a cipher name, a mode
(encrypt/decrypt), a key, and reads text from stdin or a positional argument.

### Argument parsing

```python #parse-args
import argparse
import sys

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="Classical cipher tool")
    p.add_argument("cipher", choices=["caesar", "vigenere"],
                   help="Cipher to use")
    p.add_argument("mode", choices=["encrypt", "decrypt"],
                   help="Operation mode")
    p.add_argument("key", help="Shift (integer) for Caesar, keyword for Vigenere")
    p.add_argument("text", nargs="?", default=None,
                   help="Text to process (reads stdin if omitted)")
    return p
```

### Dispatch

```python #dispatch
def run(cipher: str, mode: str, key: str, text: str) -> str:
    """Dispatch to the right cipher function."""
    if cipher == "caesar":
        shift = int(key)
        if mode == "encrypt":
            return caesar_encrypt(text, shift)
        else:
            return caesar_decrypt(text, shift)
    else:
        if mode == "encrypt":
            return vigenere_encrypt(text, key)
        else:
            return vigenere_decrypt(text, key)
```

### Main entry point

```python #cli file=cli.py
"""Command-line interface for classical ciphers."""

from ciphers import (
    caesar_encrypt, caesar_decrypt,
    vigenere_encrypt, vigenere_decrypt,
)

<<parse-args>>

<<dispatch>>

def main():
    parser = build_parser()
    args = parser.parse_args()
    text = args.text if args.text else sys.stdin.read().strip()
    print(run(args.cipher, args.mode, args.key, text))

if __name__ == "__main__":
    main()
```

## Tests

We verify round-trip correctness: encrypting then decrypting must return the
original text. We also check a few known values.

```python #tests file=test_ciphers.py
"""Tests for the cipher library."""

import unittest
from ciphers import (
    caesar_encrypt, caesar_decrypt,
    vigenere_encrypt, vigenere_decrypt,
)


class TestCaesar(unittest.TestCase):
    def test_known_value(self):
        self.assertEqual(caesar_encrypt("HELLO", 3), "KHOOR")

    def test_round_trip(self):
        for shift in range(26):
            msg = "THE QUICK BROWN FOX"
            self.assertEqual(caesar_decrypt(caesar_encrypt(msg, shift), shift), msg)

    def test_preserves_non_alpha(self):
        self.assertEqual(caesar_encrypt("HELLO, WORLD!", 5), "MJQQT, BTWQI!")


class TestVigenere(unittest.TestCase):
    def test_known_value(self):
        # Classic example: key LEMON
        self.assertEqual(
            vigenere_encrypt("ATTACKATDAWN", "LEMON"),
            "LXFOPVEFRNHR",
        )

    def test_round_trip(self):
        msg = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG"
        key = "SECRET"
        self.assertEqual(vigenere_decrypt(vigenere_encrypt(msg, key), key), msg)

    def test_preserves_non_alpha(self):
        ct = vigenere_encrypt("HELLO, WORLD!", "KEY")
        self.assertIn(",", ct)
        self.assertIn("!", ct)


if __name__ == "__main__":
    unittest.main()
```

## Usage

After tangling, try the tool:

```sh
# Encrypt with Caesar shift 13 (ROT13)
echo "HELLO WORLD" | python cli.py caesar encrypt 13

# Decrypt it back
echo "URYYB JBEYQ" | python cli.py caesar decrypt 13

# Vigenere with keyword
python cli.py vigenere encrypt SECRET "ATTACK AT DAWN"

# Decrypt
python cli.py vigenere decrypt SECRET "SXVRGD SX HSAN"
```
