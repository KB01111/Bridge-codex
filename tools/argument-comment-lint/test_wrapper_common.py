#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path
import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import wrapper_common


class WrapperCommonTest(unittest.TestCase):
    def test_defaults_to_workspace_and_all_targets(self) -> None:
        parsed = wrapper_common.parse_wrapper_args([])
        manifest_path = Path("/repo/codex-rs/Cargo.toml")
        final_args = wrapper_common.build_final_args(parsed, manifest_path)

        self.assertEqual(
            final_args,
            [
                "--manifest-path",
                str(manifest_path),
                "--workspace",
                "--no-deps",
                "--",
                "--all-targets",
            ],
        )

    def test_forwarded_cargo_args_keep_single_separator(self) -> None:
        parsed = wrapper_common.parse_wrapper_args(["-p", "codex-core", "--", "--tests"])
        manifest_path = Path("/repo/codex-rs/Cargo.toml")
        final_args = wrapper_common.build_final_args(parsed, manifest_path)

        self.assertEqual(
            final_args,
            [
                "--manifest-path",
                str(manifest_path),
                "--no-deps",
                "-p",
                "codex-core",
                "--",
                "--tests",
            ],
        )

    def test_fix_does_not_add_all_targets(self) -> None:
        parsed = wrapper_common.parse_wrapper_args(["--fix", "-p", "codex-core"])
        manifest_path = Path("/repo/codex-rs/Cargo.toml")
        final_args = wrapper_common.build_final_args(parsed, manifest_path)

        self.assertEqual(
            final_args,
            [
                "--manifest-path",
                str(manifest_path),
                "--no-deps",
                "--fix",
                "-p",
                "codex-core",
            ],
        )

    def test_explicit_manifest_and_workspace_are_preserved(self) -> None:
        parsed = wrapper_common.parse_wrapper_args(
            [
                "--manifest-path",
                "/tmp/custom/Cargo.toml",
                "--workspace",
                "--no-deps",
                "--",
                "--bins",
            ]
        )
        final_args = wrapper_common.build_final_args(parsed, Path("/repo/codex-rs/Cargo.toml"))

        self.assertEqual(
            final_args,
            [
                "--manifest-path",
                "/tmp/custom/Cargo.toml",
                "--workspace",
                "--no-deps",
                "--",
                "--bins",
            ],
        )

    def test_explicit_package_manifest_does_not_force_workspace(self) -> None:
        parsed = wrapper_common.parse_wrapper_args(
            [
                "--manifest-path",
                "/tmp/custom/Cargo.toml",
            ]
        )
        final_args = wrapper_common.build_final_args(parsed, Path("/repo/codex-rs/Cargo.toml"))

        self.assertEqual(
            final_args,
            [
                "--no-deps",
                "--manifest-path",
                "/tmp/custom/Cargo.toml",
                "--",
                "--all-targets",
            ],
        )

    def test_default_lint_env_promotes_both_strict_lints(self) -> None:
        env: dict[str, str] = {}

        wrapper_common.set_default_lint_env(env)

        self.assertEqual(
            env["DYLINT_RUSTFLAGS"],
            "-D argument-comment-mismatch "
            "-D uncommented-anonymous-literal-argument "
            "-A unknown_lints",
        )
        self.assertEqual(env["CARGO_INCREMENTAL"], "0")

    def test_windows_toolchain_environment_uses_resolved_toolchain(self) -> None:
        rustup_home = (Path.cwd() / "rustup").resolve()
        toolchain_root = rustup_home / "toolchains" / "nightly-host-triple"
        toolchain_bin = str(toolchain_root / "bin")
        rustc = str(Path(toolchain_bin) / "rustc.exe")
        env = {"PATH": os.pathsep.join(["existing", toolchain_bin])}

        with (
            mock.patch.object(wrapper_common.sys, "platform", "win32"),
            mock.patch.object(wrapper_common, "run_capture", return_value=rustc) as run_capture,
        ):
            wrapper_common.configure_windows_toolchain_env(env)

        self.assertEqual(env["PATH"], os.pathsep.join([toolchain_bin, "existing"]))
        self.assertEqual(env["RUSTUP_HOME"], str(rustup_home))
        self.assertEqual(env["RUSTUP_TOOLCHAIN"], toolchain_root.name)
        run_capture.assert_called_once_with(
            [
                "rustup",
                "which",
                "rustc",
                "--toolchain",
                wrapper_common.TOOLCHAIN_CHANNEL,
            ],
            env=env,
        )

    def test_packaged_library_keeps_host_qualified_toolchain(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            package_root = Path(temp_dir) / "argument-comment-lint"
            package_entrypoint = package_root / "bin" / "argument-comment-lint.exe"
            library_dir = package_root / "lib"
            library_dir.mkdir(parents=True)
            library_path = (
                library_dir
                / "argument_comment_lint@nightly-2025-09-18-x86_64-pc-windows-msvc.dll"
            )
            library_path.touch()

            result = wrapper_common.find_packaged_library(package_entrypoint)

        self.assertEqual(result, library_path)


if __name__ == "__main__":
    unittest.main()
