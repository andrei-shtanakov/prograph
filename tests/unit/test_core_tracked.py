"""Tests for the PyO3-exposed tracked_closure / missing_names helpers."""

from prograph import _core


def _cand(name: str, root_path: str) -> "_core.ProjectCandidate":
    return _core.ProjectCandidate(name, root_path, _core.ProjectKind.Python, [])


def test_tracked_closure_subset_and_members() -> None:
    cands = [
        _cand("arbiter", "./arbiter"),
        _cand("arbiter-core", "./arbiter/arbiter-core"),
        _cand("other", "./other"),
    ]
    assert _core.tracked_closure(cands, ["arbiter"]) == [True, True, False]


def test_missing_names_deduplicated() -> None:
    cands = [_cand("a", "./a")]
    assert _core.missing_names(cands, ["a", "ghost", "ghost"]) == ["ghost"]


def test_index_monorepo_two_arg_call_still_works(tmp_path) -> None:
    """Backward-compatible signature: tracked defaults to None (track all)."""
    proj = tmp_path / "proj"
    proj.mkdir()
    (proj / "pyproject.toml").write_text('[project]\nname="proj"\nversion="1.0"\ndependencies=[]\n')
    (tmp_path / ".prograph").mkdir()
    summary = _core.index_monorepo(str(tmp_path), str(tmp_path / ".prograph" / "graph.db"))
    assert summary.n_projects == 1
