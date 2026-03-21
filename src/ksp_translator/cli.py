"""CLI entry point for ksp-translator."""

from __future__ import annotations

import asyncio
import logging
import shutil
import sys
import zipfile
from pathlib import Path

import click
from rich.console import Console
from rich.progress import BarColumn, MofNCompleteColumn, Progress, TextColumn, TimeElapsedColumn
from rich.table import Table

from ksp_translator.classifier import Category, classify
from ksp_translator.config import KSP_GAMEDATA
from ksp_translator.cost import CostTracker
from ksp_translator.generator import generate_translated_cfg
from ksp_translator.parser import parse_loc_file
from ksp_translator.scanner import scan_gamedata
from ksp_translator.state import TranslationState
from ksp_translator.translator import translate_batch

logging.basicConfig(
    level=logging.WARNING,
    format="%(levelname)s: %(message)s",
)

console = Console()


def _resolve_gamedata(gamedata: str | None) -> Path:
    gd = gamedata or KSP_GAMEDATA
    if not gd:
        console.print("[red]Error:[/] --gamedata or KSP_GAMEDATA env var required.")
        sys.exit(1)
    p = Path(gd)
    if not p.is_dir():
        console.print(f"[red]Error:[/] GameData path not found: {p}")
        sys.exit(1)
    return p


def _output_dir(gamedata: Path) -> Path:
    return gamedata.parent / "_TranslatorOutputs"


def _backup_dir(gamedata: Path) -> Path:
    return gamedata.parent / "_TranslatorOutputs" / "_backups"


@click.group()
def main() -> None:
    """KSP Mod Localization Translator (EN -> KO).

    Workflow: scan -> translate -> apply -> (restore)
    """


@main.command()
@click.option("--gamedata", type=str, default=None, help="Path to KSP GameData directory.")
def scan(gamedata: str | None) -> None:
    """Scan GameData for translatable localization files."""
    gd = _resolve_gamedata(gamedata)

    with Progress(
        TextColumn("[bold blue]Scanning..."),
        BarColumn(),
        TimeElapsedColumn(),
        console=console,
        transient=True,
    ) as progress:
        task = progress.add_task("scan", total=None)
        files = scan_gamedata(gd)
        progress.update(task, completed=1, total=1)

    if not files:
        console.print("[yellow]No localization files found.[/]")
        return

    table = Table(title=f"Localization Files ({len(files)})")
    table.add_column("Mod / File", style="cyan")
    table.add_column("Total", justify="right")
    table.add_column("Translatable", justify="right", style="green")
    table.add_column("Skipped", justify="right", style="dim")

    for f in files:
        rel = f.relative_to(gd)
        entries = parse_loc_file(f)
        skip = 0
        translatable = 0
        for k, v in entries.items():
            if classify(k, v) == Category.SKIP:
                skip += 1
            else:
                translatable += 1
        table.add_row(str(rel), str(len(entries)), str(translatable), str(skip))

    console.print(table)


@main.command()
@click.option("--gamedata", type=str, default=None, help="Path to KSP GameData directory.")
@click.option("--mod", type=str, default=None, help="Filter by mod name (directory name).")
@click.option("--dry-run", is_flag=True, help="Show what would be translated without translating.")
@click.option("--pseudo", is_flag=True, help="Pseudo-translate (prefix with [KO]) without API calls.")
@click.option("--api-key", type=str, default=None, help="Gemini API key (overrides env var).")
def translate(
    gamedata: str | None,
    mod: str | None,
    dry_run: bool,
    pseudo: bool,
    api_key: str | None,
) -> None:
    """Translate localization files and save translated copies.

    Does NOT modify original files. Use 'apply' to swap them in.
    """
    gd = _resolve_gamedata(gamedata)
    out = _output_dir(gd)
    files = scan_gamedata(gd)

    if mod:
        files = [f for f in files if mod.lower() in f.relative_to(gd).parts[0].lower()]

    if not files:
        console.print("[yellow]No matching localization files found.[/]")
        return

    state = TranslationState()
    cost_tracker = CostTracker()
    total_translated = 0
    total_cached = 0
    output_files: list[Path] = []

    main_task_id: int | None = None

    class CostColumn(TextColumn):
        def render(self, task):  # type: ignore[override]
            if main_task_id is not None and task.id == main_task_id:
                return f"[yellow]{cost_tracker.format_cost()}[/]"
            return ""

    with Progress(
        TextColumn("[bold]{task.description}"),
        BarColumn(),
        MofNCompleteColumn(),
        CostColumn(""),
        TimeElapsedColumn(),
        console=console,
    ) as progress:
        file_task = progress.add_task("Files", total=len(files))
        main_task_id = file_task

        for f in files:
            rel = f.relative_to(gd)
            mod_name = rel.parts[0]
            progress.update(file_task, description=f"[bold]{mod_name}[/]")

            entries = parse_loc_file(f)
            to_translate: dict[str, str] = {}
            cached: dict[str, str] = {}

            for k, v in entries.items():
                cat = classify(k, v)
                if cat == Category.SKIP:
                    continue
                c = state.get_cached(k)
                if c and not state.is_file_changed(f):
                    cached[k] = c
                else:
                    to_translate[k] = v

            if dry_run:
                console.print(
                    f"  [dim]{rel}[/] - "
                    f"[green]{len(to_translate)}[/] to translate, "
                    f"[blue]{len(cached)}[/] cached"
                )
                progress.advance(file_task)
                continue

            if not to_translate and not cached:
                progress.advance(file_task)
                continue

            translated = dict(cached)
            if to_translate:
                key_task = progress.add_task(
                    f"  Translating {mod_name}",
                    total=len(to_translate),
                )

                def _on_progress(n: int, _tid=key_task) -> None:
                    progress.advance(_tid, n)

                try:
                    new_translations = asyncio.run(
                        translate_batch(
                            to_translate,
                            api_key=api_key or "",
                            pseudo=pseudo,
                            on_progress=_on_progress,
                            cost_tracker=cost_tracker,
                        )
                    )
                    translated.update(new_translations)

                    if not pseudo:
                        for k, v in new_translations.items():
                            state.set_cached(k, v)

                    for k in to_translate:
                        if k not in new_translations:
                            state.mark_failed(k, to_translate[k])

                except Exception as e:
                    progress.console.print(
                        f"  [red]Error translating {rel}: {e}[/]"
                    )
                    for k, v in to_translate.items():
                        state.mark_failed(k, v)

                progress.remove_task(key_task)

            if not translated:
                progress.advance(file_task)
                continue

            total_translated += len(to_translate)
            total_cached += len(cached)

            # Generate translated cfg alongside originals in output dir
            translated_path = out / rel
            generate_translated_cfg(f, translated, translated_path)
            output_files.append(translated_path)

            state.register_translated(f, translated_path, str(rel))
            if not pseudo:
                state.mark_file_done(f, list(translated.keys()))
            state.save()

            progress.advance(file_task)

    if dry_run:
        return

    console.print()
    table = Table(title="Translation Complete")
    table.add_column("Metric", style="bold")
    table.add_column("Value", justify="right")
    failed_count = len(state.get_failed_keys())
    table.add_row("Files processed", str(len(files)))
    table.add_row("Keys translated", f"[green]{total_translated}[/]")
    table.add_row("Keys from cache", f"[blue]{total_cached}[/]")
    if failed_count:
        table.add_row("Failed keys", f"[red]{failed_count}[/]")
    table.add_row("API cost", f"[yellow]{cost_tracker.format_cost()}[/]")
    table.add_row("Output directory", str(out))
    console.print(table)

    if cost_tracker.total_cost > 0:
        console.print(f"\n[dim]{cost_tracker.format_detail()}[/]")

    if failed_count:
        console.print(
            f"\n[yellow]Run 'ksp-translator retry' to retry {failed_count} failed key(s).[/]"
        )
    if output_files:
        console.print(
            f"\n[dim]Generated {len(output_files)} translated file(s). "
            f"Run 'ksp-translator apply' to swap them into GameData.[/]"
        )


@main.command()
@click.option("--gamedata", type=str, default=None, help="Path to KSP GameData directory.")
def apply(gamedata: str | None) -> None:
    """Apply translations by swapping translated files into GameData.

    Backs up originals first so they can be restored later.
    """
    gd = _resolve_gamedata(gamedata)
    state = TranslationState()
    translated_files = state.get_translated_files()

    if not translated_files:
        console.print("[yellow]No translated files to apply. Run 'translate' first.[/]")
        return

    if state.is_applied():
        console.print("[yellow]Translations are already applied. Run 'restore' first.[/]")
        return

    backup = _backup_dir(gd)
    backup.mkdir(parents=True, exist_ok=True)
    state.set_backup_dir(str(backup))

    applied = 0
    with Progress(
        TextColumn("[bold]Applying..."),
        BarColumn(),
        MofNCompleteColumn(),
        TimeElapsedColumn(),
        console=console,
    ) as progress:
        task = progress.add_task("apply", total=len(translated_files))

        for source_str, info in translated_files.items():
            source = Path(source_str)
            translated = Path(info["translated_path"])
            rel = info["rel_path"]

            if not source.exists():
                progress.console.print(f"  [red]Source missing: {rel}[/]")
                progress.advance(task)
                continue

            if not translated.exists():
                progress.console.print(f"  [red]Translated file missing: {rel}[/]")
                progress.advance(task)
                continue

            # Backup original
            backup_path = backup / rel
            backup_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, backup_path)

            # Replace original with translated
            shutil.copy2(translated, source)
            applied += 1
            progress.advance(task)

    state.set_applied(True)
    state.save()

    console.print(f"\n[green]Applied {applied} translated file(s).[/]")
    console.print("[dim]Originals backed up. Run 'restore' to revert.[/]")


@main.command()
@click.option("--gamedata", type=str, default=None, help="Path to KSP GameData directory.")
def restore(gamedata: str | None) -> None:
    """Restore original files from backup."""
    gd = _resolve_gamedata(gamedata)
    state = TranslationState()

    if not state.is_applied():
        console.print("[yellow]No translations are currently applied.[/]")
        return

    backup_dir_str = state.get_backup_dir()
    if not backup_dir_str:
        console.print("[red]Backup directory not found in state.[/]")
        return

    backup = Path(backup_dir_str)
    translated_files = state.get_translated_files()
    restored = 0

    with Progress(
        TextColumn("[bold]Restoring..."),
        BarColumn(),
        MofNCompleteColumn(),
        TimeElapsedColumn(),
        console=console,
    ) as progress:
        task = progress.add_task("restore", total=len(translated_files))

        for source_str, info in translated_files.items():
            source = Path(source_str)
            rel = info["rel_path"]
            backup_path = backup / rel

            if not backup_path.exists():
                progress.console.print(f"  [red]Backup missing: {rel}[/]")
                progress.advance(task)
                continue

            shutil.copy2(backup_path, source)
            restored += 1
            progress.advance(task)

    state.set_applied(False)
    state.save()

    console.print(f"\n[green]Restored {restored} original file(s).[/]")


@main.command()
def status() -> None:
    """Show translation progress and apply state."""
    state = TranslationState()
    s = state.summary()
    table = Table(title="Translation Status")
    table.add_column("Metric", style="bold")
    table.add_column("Value", justify="right")
    table.add_row("Completed files", str(s["completed_files"]))
    table.add_row("Translated files", str(s["translated_files"]))
    table.add_row("Cached translations", f"[green]{s['cached_translations']}[/]")
    table.add_row("Failed keys", f"[red]{s['failed_keys']}[/]" if s["failed_keys"] else "0")
    table.add_row(
        "Applied",
        "[green]Yes[/]" if s["applied"] else "[dim]No[/]",
    )
    console.print(table)


@main.command()
@click.option("--pseudo", is_flag=True, help="Pseudo-translate failed keys.")
@click.option("--api-key", type=str, default=None, help="Gemini API key.")
def retry(pseudo: bool, api_key: str | None) -> None:
    """Retry translation of failed keys."""
    state = TranslationState()
    failed = state.get_failed_keys()
    if not failed:
        console.print("[green]No failed keys to retry.[/]")
        return

    with Progress(
        TextColumn("[bold]Retrying..."),
        BarColumn(),
        MofNCompleteColumn(),
        TimeElapsedColumn(),
        console=console,
    ) as progress:
        task = progress.add_task("retry", total=len(failed))
        results = asyncio.run(translate_batch(failed, api_key=api_key or "", pseudo=pseudo))

        for k, v in results.items():
            state.set_cached(k, v)
            state.clear_failed(k)
            progress.advance(task)

    state.save()
    console.print(f"[green]Retranslated {len(results)} key(s).[/]")


@main.command()
def clean() -> None:
    """Clear translation cache and state."""
    state = TranslationState()
    if state.is_applied():
        console.print("[red]Translations are currently applied. Run 'restore' before cleaning.[/]")
        return
    state.clear()
    console.print("[green]Cache and state cleared.[/]")
