"""Filesystem layout for prograph runtime artefacts under a monorepo root."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class PrographPaths:
    """All filesystem paths under `<monorepo_root>/.prograph/`."""

    monorepo_root: Path

    @property
    def prograph_dir(self) -> Path:
        return self.monorepo_root / ".prograph"

    @property
    def config_path(self) -> Path:
        return self.prograph_dir / "config.toml"

    @property
    def db_path(self) -> Path:
        return self.prograph_dir / "graph.db"

    @property
    def lock_path(self) -> Path:
        return self.prograph_dir / "index.lock"

    @property
    def log_path(self) -> Path:
        return self.prograph_dir / "index.log"

    @property
    def projects_md_dir(self) -> Path:
        return self.prograph_dir / "projects"

    @property
    def contracts_md_dir(self) -> Path:
        return self.prograph_dir / "contracts"

    @property
    def gitignore_path(self) -> Path:
        return self.prograph_dir / ".gitignore"

    @property
    def index_md_path(self) -> Path:
        return self.prograph_dir / "index.md"

    @property
    def mcp_patterns_dir(self) -> Path:
        return self.prograph_dir / "mcp_patterns"

    def ensure_dirs(self) -> None:
        self.prograph_dir.mkdir(parents=True, exist_ok=True)
        self.projects_md_dir.mkdir(parents=True, exist_ok=True)
        self.contracts_md_dir.mkdir(parents=True, exist_ok=True)
        self.mcp_patterns_dir.mkdir(parents=True, exist_ok=True)

    def is_initialized(self) -> bool:
        return self.prograph_dir.is_dir() and self.config_path.is_file()
