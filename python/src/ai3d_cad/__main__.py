"""ai3d-cad CLI entry point."""
import argparse
import sys

from . import __version__, PROTOCOL_VERSION


def main():
    parser = argparse.ArgumentParser(prog="ai3d-cad")
    parser.add_argument(
        "--version", action="version",
        version=f"ai3d-cad {__version__} (protocol {PROTOCOL_VERSION})",
    )
    subparsers = parser.add_subparsers(dest="command")

    build_parser = subparsers.add_parser("build", help="Execute CAD code and produce STL")
    build_parser.add_argument("--code", required=True, help="Path to .py or .scad file")
    build_parser.add_argument("--output", required=True, help="Output STL path")
    build_parser.add_argument("--engine", choices=["cadquery", "openscad"], default="cadquery")
    build_parser.add_argument("--step", default=None, help="Optional output STEP path (CadQuery only)")

    info_parser = subparsers.add_parser("info", help="Analyze an existing STL")
    info_parser.add_argument("--input", required=True, help="Path to STL file")

    val_parser = subparsers.add_parser("validate", help="Syntax-check code without building")
    val_parser.add_argument("--code", required=True, help="Path to .py or .scad file")
    val_parser.add_argument("--engine", choices=["cadquery", "openscad"], default="cadquery")

    assemble_parser = subparsers.add_parser("assemble", help="Assemble components from a manifest")
    assemble_parser.add_argument("--manifest", required=True, help="Path to assembly manifest JSON")
    assemble_parser.add_argument("--output", required=True, help="Output STL path")
    assemble_parser.add_argument("--step", default=None, help="Optional output STEP path")

    args = parser.parse_args()

    if args.command == "build":
        from .builder import build
        sys.exit(build(args.code, args.output, args.engine, step_path=args.step))
    elif args.command == "info":
        from .analyzer import info
        sys.exit(info(args.input))
    elif args.command == "validate":
        from .builder import validate
        sys.exit(validate(args.code, args.engine))
    elif args.command == "assemble":
        from .assembler import assemble
        sys.exit(assemble(args.manifest, args.output, step_path=args.step))
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
