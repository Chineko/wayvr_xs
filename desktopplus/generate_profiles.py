"""Generate native Desktop+ presets; no capture/rendering implementation."""
import argparse
import configparser
from pathlib import Path

LAYOUTS = {
    'Dual': [0, 2560, 5120],
    'Thirds': [0, 1706, 3413, 5120],
    'Wide Center': [0, 1280, 3840, 5120],
}


def profile(layout, backend):
    boundaries = LAYOUTS[layout]
    widths = [round((b-a) * 125 / 2560) for a, b in zip(boundaries, boundaries[1:])]
    left = -(sum(widths) / 100 + 0.03 * (len(widths)-1)) / 2
    config = configparser.ConfigParser(interpolation=None)
    config.optionxform = str
    for i, (a, b) in enumerate(zip(boundaries, boundaries[1:])):
        width = widths[i] / 100
        x = left + width / 2
        left += width + 0.03
        config[f'Overlay{i}'] = {
            'Name': f'XS {layout} {i+1}', 'NameIsCustom': 'true', 'Enabled': 'true',
            'DesktopID': '0', 'CaptureSource': str(backend), 'WinRTDesktopID': '0',
            'Width': str(widths[i]), 'Curvature': '0', 'Opacity': '100', 'Brightness': '100',
            'DisplayMode': '0', 'Origin': 'Room', 'TransformLocked': 'false',
            'OffsetRight': '0', 'OffsetUp': '0', 'OffsetForward': '0',
            'CroppingEnabled': 'true', 'CroppingX': str(a), 'CroppingY': '0',
            'CroppingWidth': str(b-a), 'CroppingHeight': '1440',
            'InputEnabled': 'true', 'InputDPlusLPEnabled': 'true',
            'ShowFloatingUI': 'true', 'ShowDesktopButtons': 'false',
            'ShowActionBar': 'true', 'ActionBarOrderUseGlobal': 'true',
            'UpdateInvisible': 'false', 'GazeFade': 'false', '3DEnabled': 'false',
            'Transform': f'[1 0 0 0 0 1 0 0 0 0 1 0 {x:.6f} 1.4 -1.5 1]',
        }
    return config


def validate(path, layout, backend):
    c = configparser.ConfigParser(interpolation=None)
    c.read(path, encoding='utf-8')
    boundaries = LAYOUTS[layout]
    assert c.sections() == [f'Overlay{i}' for i in range(len(boundaries)-1)], path
    for i, (a, b) in enumerate(zip(boundaries, boundaries[1:])):
        s = c[f'Overlay{i}']
        assert (s.getint('CroppingX'), s.getint('CroppingWidth')) == (a, b-a), path
        assert s.getint('CroppingY') == 0 and s.getint('CroppingHeight') == 1440, path
        assert s.getint('CaptureSource') == backend and s.getint('WinRTDesktopID') == 0, path
        assert s.getboolean('InputEnabled') and s.getint('Width') > 0, path


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output', type=Path, default=Path(__file__).parent / 'profiles')
    p.add_argument('--check', action='store_true')
    args = p.parse_args()
    if not args.check:
        args.output.mkdir(parents=True, exist_ok=True)
    for layout in LAYOUTS:
        for name, backend in [('Graphics Capture', 1), ('Desktop Duplication', 0)]:
            path = args.output / f'XS 5120 {layout} - {name}.ini'
            if not args.check:
                with path.open('w', encoding='utf-8', newline='\n') as f:
                    f.write('; Desktop+ XS preset. Native capture and input.\n')
                    profile(layout, backend).write(f, space_around_delimiters=False)
                path.write_text(path.read_text(encoding="utf-8").rstrip() + "\n", encoding="utf-8")
            validate(path, layout, backend)
    print('Validated six native Desktop+ profiles: dual, thirds, wide center; both capture backends.')


if __name__ == '__main__':
    main()
