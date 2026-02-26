#!/usr/bin/env python3
"""
cwl2man - Convert CWL tool descriptions to bulker manifest YAML.

Parses CWL CommandLineTool files, extracts base commands and Docker images,
and outputs a bulker-compatible manifest YAML.

Dependencies: PyYAML (pyyaml)
"""

import argparse
import glob
import logging
import os
import sys

import yaml

_LOGGER = logging.getLogger(__name__)


class BaseCommandNotFoundException(Exception):
    def __init__(self, file):
        self.file = file
        super().__init__(f"Base command not found in {file}")


class ImageNotFoundException(Exception):
    def __init__(self, file):
        self.file = file
        super().__init__(f"Docker image not found in {file}")


def parse_cwl(cwl_file):
    """Parse a CWL tool description file and extract command/image info.

    Args:
        cwl_file: Path to a CWL tool description YAML file.

    Returns:
        Dict with command, docker_image, docker_command keys, or None if
        the file is not a CommandLineTool.

    Raises:
        BaseCommandNotFoundException: If baseCommand is missing.
        ImageNotFoundException: If no Docker image is found.
    """
    with open(cwl_file, "r") as f:
        yam = yaml.safe_load(f)

    if yam.get("class") != "CommandLineTool":
        _LOGGER.info("CWL file of wrong class: %s (%s)", cwl_file, yam.get("class"))
        return None

    # Extract base command
    maybe_base_command = yam.get("baseCommand")
    if maybe_base_command is None:
        _LOGGER.info("Can't find base command from %s", cwl_file)
        raise BaseCommandNotFoundException(cwl_file)

    if isinstance(maybe_base_command, list):
        base_command = maybe_base_command[0]
    else:
        base_command = maybe_base_command

    if os.path.isabs(base_command):
        _LOGGER.debug("Converting base command to relative: %s", base_command)
        base_command = os.path.basename(base_command)

    _LOGGER.debug("Base command: %s", base_command)

    # Extract Docker image
    image = None
    try:
        if "requirements" in yam:
            reqs = yam["requirements"]
            if isinstance(reqs, dict) and "DockerRequirement" in reqs:
                image = reqs["DockerRequirement"]["dockerPull"]
            elif isinstance(reqs, list):
                for req in reqs:
                    if req.get("class") == "DockerRequirement":
                        image = req["dockerPull"]

        if not image and "hints" in yam:
            hints = yam["hints"]
            if isinstance(hints, dict) and "DockerRequirement" in hints:
                image = hints["DockerRequirement"]["dockerPull"]
            elif isinstance(hints, list):
                for hint in hints:
                    if hint.get("class") == "DockerRequirement":
                        image = hint["dockerPull"]

        if not image:
            raise ImageNotFoundException(cwl_file)
    except ImageNotFoundException:
        raise
    except Exception:
        _LOGGER.info("Can't find image for %s from %s", base_command, cwl_file)
        raise ImageNotFoundException(cwl_file)

    # Handle $include references
    if str(image).startswith("$include"):
        include_data = yaml.safe_load(str(image))
        file_path = str(include_data["$include"])
        with open(os.path.join(os.path.dirname(cwl_file), file_path), "r") as f:
            image = f.read().strip()

    _LOGGER.info("Adding image %s for command %s from file %s", image, base_command, cwl_file)

    return {
        "command": base_command,
        "docker_image": image,
        "docker_command": base_command,
    }


def build_argparser():
    parser = argparse.ArgumentParser(
        description="Convert CWL tool descriptions to bulker manifest YAML."
    )
    parser.add_argument(
        "-c", "--cwl",
        required=True,
        nargs="+",
        help="One or more CWL file paths (glob patterns accepted).",
    )
    parser.add_argument(
        "-o", "--output",
        help="Output manifest file path. If omitted, prints to stdout.",
    )
    parser.add_argument(
        "-n", "--name",
        default="cwl_manifest",
        help="Manifest name (default: cwl_manifest).",
    )
    parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Enable verbose logging.",
    )
    return parser


def expand_globs(file_patterns):
    """Expand glob patterns in the file list."""
    files = []
    for pattern in file_patterns:
        expanded = glob.glob(pattern)
        if expanded:
            files.extend(expanded)
        else:
            files.append(pattern)
    return files


def main():
    parser = build_argparser()
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.WARNING,
        format="%(levelname)s: %(message)s",
    )

    cwl_files = expand_globs(args.cwl)
    manifest = {"manifest": {"name": args.name, "commands": []}}
    base_commands_not_found = []
    images_not_found = []

    for cwl_file in cwl_files:
        try:
            cmd = parse_cwl(cwl_file)
            if cmd:
                manifest["manifest"]["commands"].append(cmd)
        except BaseCommandNotFoundException as e:
            base_commands_not_found.append(e.file)
        except ImageNotFoundException as e:
            images_not_found.append(e.file)

    # Report summary
    n_commands = len(manifest["manifest"]["commands"])
    print(f"Commands added: {n_commands}", file=sys.stderr)
    if base_commands_not_found:
        print(
            f"Base command not found ({len(base_commands_not_found)}): "
            f"{base_commands_not_found}",
            file=sys.stderr,
        )
    if images_not_found:
        print(
            f"Image not found ({len(images_not_found)}): {images_not_found}",
            file=sys.stderr,
        )

    # Output manifest
    output_yaml = yaml.dump(manifest, default_flow_style=False, sort_keys=False)
    if args.output:
        with open(args.output, "w") as f:
            f.write(output_yaml)
        print(f"Manifest written to {args.output}", file=sys.stderr)
    else:
        print(output_yaml)


if __name__ == "__main__":
    main()
