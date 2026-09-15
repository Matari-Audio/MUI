# Idiomatic Rust strings, slices, ownership, and paths

The `idiomatic-rust` repository describes itself as a peer-reviewed collection of resources for concise, conventional Rust, and its string-related links point to a useful progression: a 2024 `String`/`&str` rule of thumb, older articles on `Into` and `Cow`, and a discussion of path-like arguments. The current [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) provide the stronger, general rule underneath all of them: a function should borrow when it does not need ownership, take ownership when it does, and expose generic bounds when they capture the real requirement.

## Choose the type from ownership and representation

`String` is an owned, growable, UTF-8 string. `&str` is a borrowed view of UTF-8 text; it is a pointer and a byte length and can refer to a literal, a `String`, or a substring. The standard library documents that `str::len` counts bytes, not Unicode scalar values, and that `chars()` yields scalar values, not user-perceived grapheme clusters ([`String`](https://doc.rust-lang.org/std/string/index.html), [`str`](https://doc.rust-lang.org/std/primitive.str.html)).

For a read-only function, prefer a slice:

```rust
fn first_word(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or("")
}
```

This accepts both `&str` and `&String` through deref coercion, does not move the caller's value, and returns a view without allocating. A common bad pattern is `fn first_word(text: String) -> String`: it forces a move, makes a literal caller allocate, and often leads callers to add needless `.clone()` calls. The 2024 article in the repository recommends `String` for owning struct fields, `&str` for parameters, and `&str` returns when the result is a slice of an argument ([Klabnik, “When should I use String vs &str?”](https://steveklabnik.com/writing/when-should-i-use-string-vs-str/)).

Return `String` when the function creates independent text or must outlive its input:

```rust
fn upper_first_word(text: &str) -> String {
    text.split_whitespace().next().unwrap_or("").to_uppercase()
}
```

Trying to return `&str` from `to_uppercase()` would reference a temporary allocation and cannot be correct. Conversely, returning `String` from every substring operation is correct but needlessly allocates. A borrowed result also keeps the source alive, so use an owned return when that lifetime would make the API awkward or when the result is stored elsewhere.

Use `String` in a struct when the struct owns the text. A constructor can accept either literals or an existing owned value with `impl Into<String>`:

```rust
struct User { name: String }

impl User {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
```

This is a good ownership boundary: the constructor promises to retain the name, and a caller with a `String` can transfer it without cloning. A caller with `&str` pays for one conversion exactly where ownership is established. The older repository article explains this pattern ([Radtke, accepting `String` or `&str`](https://hermanradtke.com/2015/05/06/creating-a-rust-function-that-accepts-string-or-str.html/)). For a public API, however, do not blindly add generic bounds everywhere: `impl Into<String>` can make signatures harder to read and can complicate inference. Use it when accepting owned-or-borrowed input is a real ergonomic benefit; use `&str` when the function only reads.

`Cow<'a, str>` is appropriate when the output is usually the input unchanged but occasionally needs an owned rewrite. For example, an escaping or normalization routine can borrow the fast path and allocate only on the changed path:

```rust
use std::borrow::Cow;

fn remove_spaces(text: &str) -> Cow<'_, str> {
    if text.contains(' ') {
        Cow::Owned(text.replace(' ', ""))
    } else {
        Cow::Borrowed(text)
    }
}
```

The standard [`Cow`](https://doc.rust-lang.org/std/borrow/enum.Cow.html) contract is clone-on-write: immutable access works for either variant, while `to_mut()` or `into_owned()` materializes data when necessary. This is useful when avoiding a common allocation matters and the caller benefits from one uniform return type. It is a poor default for ordinary functions: the enum and lifetime are extra API complexity, and a straightforward `String` may be clearer. The linked 2015 article correctly identifies the conditional-allocation use case ([returning `&str` or `String`](https://hermanradtke.com/2015/05/29/creating-a-rust-function-that-returns-string-or-str.html/)), but its wording that `String` is a vector of “UTF-8 code points” is outdated/inaccurate; current documentation describes a UTF-8 encoded growable string. Treat `String` as bytes with a UTF-8 invariant, not as an indexable `Vec<char>`.

## Text is not bytes, and bytes are not characters

Do not index a `str` by an arbitrary integer. UTF-8 code points have variable byte width, so slicing at a non-boundary panics (or becomes unsafe if checks are bypassed). Use `split`, `char_indices`, `get`, or a parser that understands the intended encoding. If the protocol is binary, use `&[u8]`/`Vec<u8>` instead of forcing bytes through `String`. If the requirement is user-visible character counting or truncation, remember that `char` is only a Unicode scalar value; grapheme segmentation requires a Unicode-aware crate.

## Paths need OS-native types

`Path`/`PathBuf` are to filesystem paths what `str`/`String` are to text: `Path` is a borrowed slice and `PathBuf` owns mutable path storage. They wrap `OsStr`/`OsString`, because filenames and command-line arguments may contain platform-native sequences that are not valid UTF-8. The standard [`std::path` documentation](https://doc.rust-lang.org/std/path/index.html) explicitly describes this relationship and cross-platform behavior.

Borrow for inspection:

```rust
use std::path::Path;

fn has_config_extension(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
}
```

Store or return `PathBuf` when the value must own its path. For a filesystem-facing function that only needs a path reference, `P: AsRef<Path>` (or `impl AsRef<Path>`) is often the ergonomic boundary:

```rust
fn read_config(path: impl AsRef<Path>) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}
```

This accepts `&Path`, `PathBuf`, `String`, and string literals without making callers convert first, matching the standard library's `File::open` style. The API Guidelines explain both caller-controlled ownership and the `AsRef<Path>` pattern ([flexibility](https://rust-lang.github.io/api-guidelines/flexibility.html), [conversion traits](https://rust-lang.github.io/api-guidelines/interoperability.html)). `AsRef` is for cheap reference-to-reference conversion and must not fail; use `From`/`TryFrom` or a dedicated method for expensive or fallible conversions ([`AsRef`](https://doc.rust-lang.org/std/convert/trait.AsRef.html)).

A bad path API is `fn open(path: String)` followed by `Path::new(&path)`: it moves text unnecessarily and can never represent a non-Unicode filename. Another bad pattern is `path.to_str().unwrap()` in general filesystem code. `to_str()` can return `None`; propagate an error if Unicode is required, or use `path.display()`/`to_string_lossy()` only for human-facing diagnostics, where replacement characters are acceptable. Use `OsStr`/`OsString` directly when passing names to OS APIs and preserve the platform representation until a textual boundary is explicitly required ([`OsStr`](https://doc.rust-lang.org/std/ffi/struct.OsStr.html), [`PathBuf`](https://doc.rust-lang.org/std/path/struct.PathBuf.html)).

These choices are not style points detached from correctness. Borrowing prevents accidental moves and clones; owned values make lifetimes and retention explicit; `Cow` expresses a measurable conditional-allocation trade-off; `Path` and `OsStr` preserve filenames that `String` cannot represent. Apply the simplest type that states the function's real ownership, encoding, and lifetime contract.
