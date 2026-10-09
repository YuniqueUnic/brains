"""Merge xidlc types/client output and keep generated Rust modules focused."""

from pathlib import Path
import argparse
import re
import shutil


def module_body(path: Path) -> str:
    text = path.read_text()
    start = text.index("pub mod ")
    open_brace = text.index("{", start)
    depth = 0
    for index in range(open_brace, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[open_brace + 1 : index]
    raise SystemExit(f"unbalanced module in {path}")


def matching_brace(text: str, start: int) -> int:
    opening = text.index("{", start)
    depth = 0
    in_string = False
    escaped = False
    line_comment = False
    block_depth = 0
    index = opening
    while index < len(text):
        char = text[index]
        following = text[index + 1] if index + 1 < len(text) else ""
        if line_comment:
            line_comment = char != "\n"
        elif block_depth:
            if char == "/" and following == "*":
                block_depth += 1
                index += 1
            elif char == "*" and following == "/":
                block_depth -= 1
                index += 1
        elif in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == "/" and following == "/":
            line_comment = True
            index += 1
        elif char == "/" and following == "*":
            block_depth = 1
            index += 1
        elif char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    raise SystemExit("unbalanced Rust block while splitting generated SDK")


def block(text: str, marker: str, start: int = 0) -> tuple[int, int]:
    begin = text.index(marker, start)
    return begin, matching_brace(text, begin) + 1


def patch_optional_headers(text: str) -> str:
    optional_headers = {
        "if_match",
        "if_none_match",
        "operation_id",
        "content_sha256",
        "csrf_token",
    }
    lines = text.split("\n")
    output = []
    index = 0
    while index < len(lines):
        line = lines[index]
        output.append(line)
        param = line.strip().removeprefix("let header_value_str = ").removesuffix(";").strip()
        if param in optional_headers and not (
            index + 1 < len(lines)
            and lines[index + 1].strip().startswith("if let Some(header_value_str)")
        ):
            insert = index + 1
            while insert < len(lines) and "req.headers_mut().insert(header_name, header_value);" not in lines[insert]:
                insert += 1
            if insert < len(lines):
                indent = " " * 12
                output.append(indent + "if let Some(header_value_str) = header_value_str.clone() {")
                output.extend(indent + lines[row] for row in range(index + 1, insert + 1))
                output.append(indent + "}")
                index = insert + 1
                continue
        index += 1
    return "\n".join(output)


def method_chunks(operations: str) -> dict[str, str]:
    methods = list(re.finditer(r"^\s*pub async fn ", operations, re.MULTILINE))
    chunks = {}
    starts = []
    for method in methods:
        attribute = operations.rfind("\n        #[allow(", 0, method.start())
        starts.append(attribute + 1 if attribute >= 0 else method.start())
    for index, start in enumerate(starts):
        end = starts[index + 1] if index + 1 < len(starts) else len(operations)
        chunk = operations[start:end].strip()
        signature = re.search(r"pub async fn ([^ (]+)", chunk)
        if signature is None:
            raise SystemExit("could not read generated client method name")
        chunks[signature.group(1)] = chunk
    return chunks


def generated_tree(module: str, header: str) -> None:
    files_marker = module.index("pub(super) mod __xidl_in_Files")
    files_start = module.rfind("#[allow(non_snake_case, unused_imports)]", 0, files_marker)
    if files_start < 0:
        files_start = files_marker
    session_start = module.index("pub struct SessionClient")
    types = module[:files_start].strip()
    files = module[files_start:session_start].strip()
    session = module[session_start:].strip()

    files_struct_start, files_struct_end = block(files, "pub struct FilesClient")
    request_types = files[:files_struct_start].strip()
    struct_and_impl_start, struct_and_impl_end = files_struct_start, files_struct_end
    constructor_start, constructor_end = block(files, "impl FilesClient", struct_and_impl_end)
    operations_start, operations_end = block(files, "impl FilesClient", constructor_end)
    operations = files[operations_start:operations_end]
    operations_open = operations.index("{")
    operations_close = matching_brace(operations, 0)
    chunks = method_chunks(operations[operations_open + 1 : operations_close])

    read_names = {
        "capabilities",
        "list_files",
        "get_file_meta",
        "list_revisions",
        "search",
        "links",
        "diff",
    }
    write_names = {
        "put_file",
        "delete_file",
        "patch_file",
        "restore",
        "copy",
        "r#move",
        "reindex",
    }
    expected_names = read_names | write_names
    unknown = set(chunks) - expected_names
    if unknown:
        raise SystemExit(f"unclassified generated client methods: {sorted(unknown)}")
    missing = expected_names - set(chunks)
    if missing:
        raise SystemExit(f"missing generated client methods: {sorted(missing)}")

    root = Path("sdk/rust/src/brains_generated.rs")
    module_dir = Path("sdk/rust/src/brains_generated")
    if module_dir.exists():
        shutil.rmtree(module_dir)
    (module_dir / "brains" / "files").mkdir(parents=True)
    (module_dir / "brains.rs").write_text(
        header
        + "mod files;\nmod session;\nmod types;\n\n"
        + "pub use files::*;\npub use session::*;\npub use types::*;\n"
    )
    (module_dir / "brains" / "types.rs").write_text(header + types + "\n")

    client_struct = files[struct_and_impl_start:struct_and_impl_end]
    constructors = files[constructor_start:constructor_end]
    files_source = (
        header
        + "use super::types::*;\n\n"
        + request_types
        + "\n\n"
        + client_struct
        + "\n\n"
        + constructors
        + "\n\nmod read;\nmod write;\n"
    )
    (module_dir / "brains" / "files.rs").write_text(files_source)

    def impl_source(names: set[str]) -> str:
        selected = [chunks[name] for name in chunks if name in names]
        return header + "use super::*;\n\n#[allow(non_snake_case)]\nimpl FilesClient {\n" + "\n\n".join(selected) + "\n}\n"

    (module_dir / "brains" / "files" / "read.rs").write_text(impl_source(read_names))
    (module_dir / "brains" / "files" / "write.rs").write_text(
        impl_source(write_names)
    )
    (module_dir / "brains" / "session.rs").write_text(
        header + "use super::types::*;\n\n" + session + "\n"
    )

    oversized = [
        (str(path), len(path.read_text().splitlines()))
        for path in module_dir.rglob("*.rs")
        if len(path.read_text().splitlines()) > 800
    ]
    if oversized:
        raise SystemExit(f"generated Rust SDK file exceeds 800 lines: {oversized}")
    root.write_text(header + "pub mod brains;\n")
    print("split generated Rust SDK:", ", ".join(f"{path} ({lines})" for path, lines in sorted(
        (str(path.relative_to(Path("sdk/rust/src"))), len(path.read_text().splitlines()))
        for path in module_dir.rglob("*.rs")
    )))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--split-existing", action="store_true")
    args = parser.parse_args()
    source = Path("sdk/rust/src/brains_generated.rs")

    if args.split_existing:
        current = source.read_text()
        body = "\n".join(
            line for line in current.splitlines() if line.strip() and not line.startswith("//")
        )
        split_files = [
            Path("sdk/rust/src/brains_generated/brains.rs"),
            Path("sdk/rust/src/brains_generated/brains/types.rs"),
            Path("sdk/rust/src/brains_generated/brains/files.rs"),
            Path("sdk/rust/src/brains_generated/brains/files/read.rs"),
            Path("sdk/rust/src/brains_generated/brains/files/write.rs"),
            Path("sdk/rust/src/brains_generated/brains/session.rs"),
        ]
        if body == "pub mod brains;":
            if all(path.is_file() for path in split_files):
                print("generated Rust SDK is already split")
                return
            raise SystemExit(
                "generated Rust SDK split tree is incomplete; regenerate it from api/brains.idl"
            )
        header_lines = [
            line for line in current.splitlines()[:3] if line.startswith("// Code generated")
        ]
        header = "\n".join(header_lines) + "\n\n"
        generated_tree(patch_optional_headers(module_body(source)), header)
        return

    types_file = next(Path("/tmp/brains-sdk-types").glob("*.rs"))
    client_file = Path("/tmp/brains-sdk-rs/brains.rs")
    client = client_file.read_text()
    client_module = module_body(client_file)
    if "pub struct CapabilityLimits" not in client_module:
        client_module = types_file.read_text().replace(
            "pub mod brains {\n", "pub mod brains {\n" + module_body(types_file) + "\n\n", 1
        )
        client_module = module_body_from_text(client_module)
    client_module = patch_optional_headers(client_module)
    version_line = next(
        (line for line in client.splitlines() if line.startswith("// Code generated by xidlc-v")),
        "// Code generated by xidlc. DO NOT EDIT.",
    )
    header = (
        "// Code generated from api/brains.idl by scripts/gen-api.sh. DO NOT EDIT.\n"
        + version_line
        + "\n\n"
    )
    generated_tree(client_module, header)


def module_body_from_text(text: str) -> str:
    start = text.index("pub mod brains")
    return extract_delimited_module(text, start)


def extract_delimited_module(text: str, start: int) -> str:
    open_brace = text.index("{", start)
    end = matching_brace(text, open_brace)
    return text[open_brace + 1 : end]


if __name__ == "__main__":
    main()
