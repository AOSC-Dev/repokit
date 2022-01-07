#!/bin/bash -e

CURDIR="$(dirname "$0")"
echo "Running migrations ..."
sqlite3 verify.db < "$CURDIR/../repo-notifier/migrations/"*.sql
echo "... Done."
