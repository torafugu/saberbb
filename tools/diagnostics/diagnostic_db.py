"""Shared CLI-only database path default; run() APIs remain explicit."""
import os
from pathlib import Path

DB_ENV = 'SABERBB_DB'


def add_db_argument(parser, flag='--db', required=True):
    value = os.environ.get(DB_ENV)
    default = Path(value).expanduser() if value and value.strip() else None
    parser.add_argument(flag, type=lambda x: Path(x).expanduser(), default=default,
        required=required and default is None,
        help=f'SQLite file path; defaults to ${DB_ENV}; explicit {flag} takes precedence')
