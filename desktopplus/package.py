"""Package the native binaries and their complete corresponding source."""
import argparse
import hashlib
import json
import subprocess
import zipfile
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--source', type=Path, required=True)
p.add_argument('--destination', type=Path, default=Path(__file__).parent / 'dist')
args = p.parse_args()
source = args.source.resolve()
recipe = Path(__file__).resolve().parent
destination = args.destination.resolve()
destination.mkdir(parents=True, exist_ok=True)
release = source / 'src/x64/Release'
binaries = ['DesktopPlus.exe', 'DesktopPlusUI.exe', 'DesktopPlusWinRT.dll']
for name in binaries:
    if not (release / name).is_file():
        raise SystemExit(f'Missing built binary: {release / name}')

tracked = subprocess.check_output(['git', '-C', str(source), 'ls-files', '-z']).decode().split('\0')
tracked = [s for s in tracked if s]
profiles = sorted((recipe / 'profiles').glob('XS*.ini'))
assert len(profiles) == 12
readme = (recipe / 'README.md').read_bytes()
patch_sha = hashlib.sha256((recipe / 'xs-native-controls.patch').read_bytes()).hexdigest()
manifest = {
    'version': 'Desktop+ XS 0.2.0',
    'upstream': 'https://github.com/elvissteinjr/DesktopPlus',
    'upstream_commit': '031d74d69f08411a9a9b1030d3d428ac02b93549',
    'patch_sha256': patch_sha,
    'recipe_repository': 'https://github.com/Chineko/wayvr_xs/tree/feat/windows-steamvr/desktopplus',
    'binaries': {name: hashlib.sha256((release / name).read_bytes()).hexdigest() for name in binaries},
}

binary_zip = destination / 'DesktopPlus-XS-0.2.0-windows-x64.zip'
with zipfile.ZipFile(binary_zip, 'w', compression=zipfile.ZIP_DEFLATED) as z:
    for path in tracked:
        if path.startswith('assets/'):
            z.write(source / path, 'DesktopPlus-XS/' + path.removeprefix('assets/'))
    for profile in profiles:
        z.write(profile, 'DesktopPlus-XS/profiles/' + profile.name)
    for name in binaries:
        z.write(release / name, 'DesktopPlus-XS/' + name)
    z.write(source / 'LICENSE', 'DesktopPlus-XS/LICENSE')
    z.writestr('DesktopPlus-XS/XS-README.md', readme)
    z.writestr('DesktopPlus-XS/XS-VERSION.json', json.dumps(manifest, indent=2))

source_zip = destination / 'DesktopPlus-XS-0.2.0-source.zip'
with zipfile.ZipFile(source_zip, 'w', compression=zipfile.ZIP_DEFLATED) as z:
    for path in tracked:
        z.write(source / path, 'DesktopPlus-XS-source/' + path)
    for profile in profiles:
        z.write(profile, 'DesktopPlus-XS-source/assets/profiles/' + profile.name)
    for path in recipe.rglob('*'):
        relative = path.relative_to(recipe)
        if path.is_file() and not any(part.startswith('.') or part in ('dist', '__pycache__') for part in relative.parts):
            z.write(path, 'DesktopPlus-XS-source/XS-build-recipe/' + relative.as_posix())
    z.writestr('DesktopPlus-XS-source/XS-VERSION.json', json.dumps(manifest, indent=2))

with zipfile.ZipFile(binary_zip) as z:
    assert 'DesktopPlus-XS/openvr_api.dll' in z.namelist(), 'Missing upstream OpenVR runtime'
    for name in binaries:
        assert z.read('DesktopPlus-XS/' + name) == (release / name).read_bytes()
print(json.dumps({'binary': str(binary_zip), 'source': str(source_zip), 'version': manifest['version']}))
