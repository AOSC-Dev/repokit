#!/usr/bin/python3

# Scan topics - delete topic debs if no corresponding branch exists on GH.
#
# Called periodically from /usr/local/bin/repo-clean-up

import logging
import os
import shutil
import sys
from pathlib import PosixPath

import requests


def collect_all_branches():
    page = 1
    branches = []
    while True:
        logging.info("Reading page %s ...", page)
        resp = requests.get(
            f"https://api.github.com/repos/AOSC-Dev/aosc-os-abbs/branches?per_page=100&page={page}",
            headers={"Authorization": f"bearer {os.environ['GITHUB_TOKEN']}"},
            timeout=30,  # 30 seconds timeout
        )
        resp.raise_for_status()
        b = resp.json()
        branches.extend(b)
        if len(b) == 100:
            page += 1
            continue
        break
    return branches


def do_clean_up(root_path: PosixPath) -> None:
    if not root_path.is_dir():
        raise NotADirectoryError(20, "Root path is not a directory", root_path)
    logging.basicConfig(level=logging.INFO)
    topics = os.listdir(root_path)
    logging.info("Reading topics list ...")
    branches = collect_all_branches()
    logging.info("Done reading topics list.")
    branches_lookup = {i["name"] for i in branches}
    logging.info("Found %s branches.", len(branches_lookup))
    closed = []
    for topic in topics:
        if topic == "stable" or topic.startswith((".", "bsp-")):
            continue
        topic_path = root_path.joinpath(topic)
        if not topic_path.is_dir():
            continue
        if topic not in branches_lookup:
            deprecated_marker = topic_path.joinpath("DEPRECATED")
            if not deprecated_marker.is_file():
                with deprecated_marker.open("wb") as f:
                    f.write(b"WARNING: This topic will be deleted.\n")
                logging.info("Warning marker set: %s", topic)
                continue
            closed.append(topic)
    for pr in closed:
        shutil.rmtree(root_path.joinpath(pr))
        logging.info("Deleted: %s", pr)


def main():
    for path in sys.argv[1:]:
        root_path = PosixPath(path)
        try:
            do_clean_up(root_path)
        except Exception as e:
            logging.exception("Error cleaning up: %s", e)


if __name__ == "__main__":
    main()
