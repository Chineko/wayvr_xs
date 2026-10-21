import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

loader = importlib.machinery.SourceFileLoader(
    'support', str(Path(__file__).resolve().parents[1] / 'wayvr-xs-support')
)
spec = importlib.util.spec_from_loader(loader.name, loader)
support = importlib.util.module_from_spec(spec)
loader.exec_module(support)


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / 'settings/wayvr'
        self.root.mkdir(parents=True)
        self.env = patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.root.parent)})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.stopped = patch.object(support, 'running', return_value=False)
        self.stopped.start()
        self.addCleanup(self.stopped.stop)
        self.output = contextlib.redirect_stdout(io.StringIO())
        self.output.__enter__()
        self.addCleanup(self.output.__exit__, None, None, None)

    def test_backup_restore_keeps_previous_workspace_and_links(self):
        (self.root / 'layouts.json').write_text('original layout')
        secret = self.base / 'outside'
        secret.write_text('outside data')
        (self.root / 'link').symlink_to(secret)
        saved = support.backup(self.base / 'backups')
        self.assertEqual(saved.stat().st_mode & 0o777, 0o700)
        self.assertTrue((saved / 'config/link').is_symlink())
        (self.root / 'layouts.json').write_text('newer layout')
        support.restore(saved)
        self.assertEqual((self.root / 'layouts.json').read_text(), 'original layout')
        previous = next(self.root.parent.glob('wayvr-xs-before-restore-*'))
        self.assertEqual((previous / 'config/layouts.json').read_text(), 'newer layout')
        self.assertEqual(secret.read_text(), 'outside data')
        support.restore(previous)
        self.assertEqual((self.root / 'layouts.json').read_text(), 'newer layout')

    def test_rejects_nested_backup_running_app_and_invalid_backup(self):
        with self.assertRaises(ValueError):
            support.backup(self.root / 'backup')
        with patch.object(support, 'running', return_value=True):
            with self.assertRaises(ValueError):
                support.backup(self.base / 'backup')
        saved = support.backup(self.base / 'backup')
        (saved / 'backup.json').write_text(json.dumps({'format': 999}))
        with self.assertRaises(ValueError):
            support.restore(saved)
        self.assertTrue(self.root.is_dir())

    def test_restore_rolls_back_failed_install_and_copy_failure_keeps_live_data(self):
        (self.root / 'state').write_text('old')
        saved = support.backup(self.base / 'backups')
        (self.root / 'state').write_text('current')
        real_rename = Path.rename
        def fail_install(path, target):
            if path.parent.name.startswith('.wayvr-xs-restore-'):
                raise OSError('simulated full filesystem')
            return real_rename(path, target)
        with patch.object(Path, 'rename', fail_install):
            with self.assertRaises(OSError):
                support.restore(saved)
        self.assertEqual((self.root / 'state').read_text(), 'current')
        with patch.object(support.shutil, 'copytree', side_effect=OSError('copy failed')):
            with self.assertRaises(OSError):
                support.restore(saved)
        self.assertEqual((self.root / 'state').read_text(), 'current')

    def test_diagnostics_exclude_config_and_bound_log_size(self):
        (self.root / 'private-layout').write_text('private')
        logfile = self.base / 'app.log'
        logfile.write_bytes(b'x' * (5 * 1024 * 1024))
        with patch.object(support, 'doctor', return_value='report'):
            support.diagnose(self.base / 'reports', logfile)
        report = next((self.base / 'reports').iterdir())
        self.assertEqual({p.name for p in report.iterdir()}, {'report.txt', 'wayvr.log'})
        self.assertEqual((report / 'wayvr.log').stat().st_size, 4 * 1024 * 1024)
        self.assertEqual(report.stat().st_mode & 0o777, 0o700)


if __name__ == '__main__':
    unittest.main()
