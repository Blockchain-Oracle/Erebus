"""Publish, search and resolve counterparties from their real signed service descriptors.

`publish` indexes one seller's `<role>.descriptor.json` (written by `erebus-negotiate
prepare_operator`) under a capability and an advertised unit-price band. `search` lists current
records cheapest first, filtered by asset, ceiling and settlement mode. `resolve` returns the
record a buyer operator configures from -- the descriptor file becomes its `peer_descriptor_file`
-- and refuses an expired or swapped descriptor. Signature verification happens in Rust when the
negotiation config loads that file; this index never claims it.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

from erebus_agents.discovery import DiscoveryDirectory, ServiceRecord


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True, help="the owner-only discovery index file")
    commands = parser.add_subparsers(dest="command", required=True)
    publish = commands.add_parser("publish")
    publish.add_argument("--service-id", required=True)
    publish.add_argument("--capability", required=True)
    publish.add_argument("--min-unit-price", type=int, required=True)
    publish.add_argument("--max-unit-price", type=int, required=True)
    publish.add_argument("--descriptor", type=Path, required=True, help="absolute path to the signed descriptor")
    search = commands.add_parser("search")
    search.add_argument("--capability", required=True)
    search.add_argument("--asset", help="CAIP-19 asset the seller must accept")
    search.add_argument("--max-unit-price", type=int)
    search.add_argument("--mode", choices=["public-bound", "shielded"])
    resolve = commands.add_parser("resolve")
    resolve.add_argument("--service-id", required=True)
    args = parser.parse_args()
    now = int(time.time())
    directory = DiscoveryDirectory.load(args.directory) if args.directory.exists() else DiscoveryDirectory(args.directory)
    if args.command == "publish":
        record = ServiceRecord.from_descriptor(args.service_id, args.capability, args.min_unit_price,
                                               args.max_unit_price, args.descriptor)
        directory.publish(record)
        directory.save()
        output: object = record.to_dict()
    elif args.command == "search":
        output = [record.to_dict() for record in directory.search(
            args.capability, now=now, asset=args.asset, max_unit_price=args.max_unit_price, mode=args.mode)]
    else:
        output = directory.resolve(args.service_id, now=now).to_dict()
    json.dump(output, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
