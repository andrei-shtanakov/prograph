"""Filesystem layout for prograph runtime artefacts under a monorepo root."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class PrographPaths:
    """All filesystem paths under `<monorepo_root>/.prograph/`.

    Internal artefacts (db, config, lock, log, mcp_patterns) always live under
    `.prograph/`. Markdown export (`projects/`, `contracts/`, `index.md`) lives
    under `export_root` when set, otherwise under `.prograph/`. A relative
    `export_root` resolves against `monorepo_root`; an absolute one is used as-is.
    """

    monorepo_root: Path
    export_root: Path | None = None

    @property
    def prograph_dir(self) -> Path:
        return self.monorepo_root / ".prograph"

    @property
    def _export_base(self) -> Path:
        if self.export_root is None:
            return self.prograph_dir
        er = self.export_root
        return er if er.is_absolute() else (self.monorepo_root / er)

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
        return self._export_base / "projects"

    @property
    def contracts_md_dir(self) -> Path:
        return self._export_base / "contracts"

    @property
    def gitignore_path(self) -> Path:
        return self.prograph_dir / ".gitignore"

    @property
    def index_md_path(self) -> Path:
        return self._export_base / "index.md"

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
