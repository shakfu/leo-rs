# leo-markdown

Markdown, Quarto and R Markdown files as [leo-rs](https://github.com/shakfu/leo-rs) outline trees. Headings become nodes; executable cells and labelled fences become child nodes headlined `<< name >>`. The writer reproduces the file byte for byte, except CRLF line endings and a byte-order mark, which a read reports.

It provides the `@qmd` and `@rmd` kinds for `leolib::ext`. leotui and leogui register them through `leo-plugins`. What they do: [docs/plugins.md](https://github.com/shakfu/leo-rs/blob/main/docs/plugins.md).

License: MIT.
