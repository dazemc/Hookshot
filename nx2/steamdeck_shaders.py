"""Prepare shaders on an x86-64 Linux PC with a Steam Deck's own RADV driver.

The driver and its DRM shim stay in the builder cache. Only the resulting game
caches travel with the game. No compatibility header is rewritten. SSH must
already work without an interactive password prompt.

    python -I nx2/steamdeck_shaders.py --game <game folder> --deck deck@steamdeck
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import uuid


class DeckError(RuntimeError):
    pass


# Runs in a fresh process: LD_PRELOAD must be present before libvulkan loads.
# VkPhysicalDeviceProperties has a fixed prefix through pipelineCacheUUID.
VULKAN_QUERY = r'''
import ctypes as C, json
class App(C.Structure):
    _fields_ = [('type', C.c_uint32), ('next', C.c_void_p), ('name', C.c_char_p),
                ('version', C.c_uint32), ('engine', C.c_char_p), ('engine_version', C.c_uint32), ('api', C.c_uint32)]
class Info(C.Structure):
    _fields_ = [('type', C.c_uint32), ('next', C.c_void_p), ('flags', C.c_uint32), ('app', C.POINTER(App)),
                ('layers', C.c_uint32), ('layer_names', C.c_void_p), ('extensions', C.c_uint32), ('extension_names', C.c_void_p)]
vk = C.CDLL('libvulkan.so.1')
vk.vkCreateInstance.argtypes = [C.POINTER(Info), C.c_void_p, C.POINTER(C.c_void_p)]
vk.vkEnumeratePhysicalDevices.argtypes = [C.c_void_p, C.POINTER(C.c_uint32), C.c_void_p]
vk.vkGetPhysicalDeviceProperties.argtypes = [C.c_void_p, C.c_void_p]
vk.vkDestroyInstance.argtypes = [C.c_void_p, C.c_void_p]
app = App(0, None, b'Hookshot Deck preparation', 0, None, 0, (1 << 22) | (3 << 12))
info = Info(1, None, 0, C.pointer(app), 0, None, 0, None)
instance = C.c_void_p()
code = vk.vkCreateInstance(C.byref(info), None, C.byref(instance))
if code: raise RuntimeError('Vulkan initialization failed (%d)' % code)
try:
    count = C.c_uint32()
    code = vk.vkEnumeratePhysicalDevices(instance, C.byref(count), None)
    if code or not count.value: raise RuntimeError('No Vulkan device')
    devices = (C.c_void_p * count.value)()
    code = vk.vkEnumeratePhysicalDevices(instance, C.byref(count), devices)
    if code: raise RuntimeError('Vulkan device enumeration failed (%d)' % code)
    result = []
    for device in devices:
        properties = C.create_string_buffer(4096)
        vk.vkGetPhysicalDeviceProperties(device, properties)
        words = C.cast(properties, C.POINTER(C.c_uint32))
        result.append(dict(api=words[0], driver=words[1], vendor=words[2], device=words[3],
                           name=properties.raw[20:276].split(b'\0', 1)[0].decode('utf8'),
                           cache_uuid=properties.raw[276:292].hex()))
finally:
    vk.vkDestroyInstance(instance, None)
print(json.dumps(result))
'''

REMOTE_SNAPSHOT = VULKAN_QUERY.replace('print(json.dumps(result))', r'''
import hashlib, pathlib, platform, re, subprocess
if platform.machine() != 'x86_64': raise RuntimeError('The Deck must use x86-64 Linux')
profile = next((d for d in result if d['vendor'] == 0x1002 and d['device'] in (0x163f, 0x1435)), None)
if profile is None: raise RuntimeError('No supported Steam Deck Vangogh device found')
names = ['libvulkan_radeon.so', 'libamdgpu_noop_drm_shim.so']
paths = {n: pathlib.Path('/usr/lib') / n for n in names}
for name in names:
    listing = subprocess.check_output(['ldd', str(paths[name])], text=True)
    for line in listing.splitlines():
        match = re.search(r'=> (/usr/lib/[^ ]+)', line)
        if match:
            path = pathlib.Path(match[1])
            if path.name not in ('libc.so.6', 'libm.so.6', 'libpthread.so.0', 'libdl.so.2', 'librt.so.1'):
                paths[path.name] = path
files = []
for name, path in sorted(paths.items()):
    data = path.read_bytes()
    files.append(dict(name=name, size=len(data), sha256=hashlib.sha256(data).hexdigest()))
print(json.dumps(dict(profile=profile, files=files)))
''')


def target(value):
    """An SSH destination, never command-line switches or shell syntax."""
    value = value.strip()
    if not re.fullmatch(r'(?:[A-Za-z0-9_][A-Za-z0-9_.-]*@)?[A-Za-z0-9][A-Za-z0-9_.-]*', value):
        raise DeckError('Enter the Deck SSH address, for example deck@steamdeck or deck@192.168.1.20.')
    return value


def ssh_command(deck, code):
    ssh = shutil.which('ssh')
    if not ssh:
        raise DeckError('Steam Deck shader preparation needs the OpenSSH client on this PC.')
    config = ['-F', os.environ['HOOKSHOT_SSH_CONFIG']] if os.environ.get('HOOKSHOT_SSH_CONFIG') else []
    return [ssh, *config, '-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '--', target(deck),
            'python3 -c ' + shlex.quote(code)]


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def validate_manifest(manifest):
    try:
        profile, files = manifest['profile'], manifest['files']
        if profile['vendor'] != 0x1002 or profile['device'] not in (0x163f, 0x1435):
            raise ValueError('unsupported GPU')
        if not re.fullmatch('[0-9a-f]{32}', profile['cache_uuid']):
            raise ValueError('invalid pipeline cache UUID')
        if not isinstance(files, list) or not 2 <= len(files) <= 128:
            raise ValueError('invalid driver file list')
        names = set()
        for item in files:
            name = item['name']
            if not re.fullmatch(r'lib[A-Za-z0-9_+.-]+\.so(?:\.[A-Za-z0-9_.+-]+)*', name) or name in names:
                raise ValueError('invalid or duplicate library name')
            if name in ('libc.so.6', 'libm.so.6', 'libpthread.so.0', 'libdl.so.2', 'librt.so.1'):
                raise ValueError('the host C runtime must stay host-provided')
            names.add(name)
            if not isinstance(item['size'], int) or not 0 < item['size'] <= 512 * 1024 * 1024:
                raise ValueError('invalid library size')
            if not re.fullmatch('[0-9a-f]{64}', item['sha256']):
                raise ValueError('invalid library checksum')
        if not {'libvulkan_radeon.so', 'libamdgpu_noop_drm_shim.so'} <= names:
            raise ValueError('the driver or DRM shim is missing')
    except (KeyError, TypeError, ValueError) as error:
        raise DeckError('The Deck returned an invalid driver snapshot: %s' % error) from error
    return manifest


def driver_environment(bundle, base=None):
    bundle = Path(bundle).resolve()
    env = dict(os.environ if base is None else base)
    for name in list(env):
        if name.startswith(('VK_', 'RADV_', 'MESA_', 'AMD_')) or name in ('DISPLAY', 'WAYLAND_DISPLAY'):
            del env[name]
    env.update(LD_LIBRARY_PATH=str(bundle), LD_PRELOAD=str(bundle / 'libamdgpu_noop_drm_shim.so'),
               AMDGPU_GPU_ID='vangogh', VK_DRIVER_FILES=str(bundle / 'radv-deck.json'),
               HOS_HEADLESS='1', HOS_AUDIO='0', HOOKSHOT_NO_UPDATE_CHECK='1',
               XDG_CACHE_HOME=str(bundle / 'cache'),
               MESA_DISK_CACHE_SINGLE_FILE='0', MESA_DISK_CACHE_DATABASE='0', MESA_DISK_CACHE_MULTI_FILE='1',
               HOOKSHOT_DECK_PREPARE='1', RX_VK_PREPARE_AGAIN='1')
    return env


def check_driver(bundle, manifest):
    """The copied driver must expose the actual Deck's identity, including its UUID."""
    try:
        probe = subprocess.run([sys.executable, '-I', '-c', VULKAN_QUERY], env=driver_environment(bundle),
                               capture_output=True, text=True, timeout=30, check=True)
        devices = json.loads(probe.stdout)
        if manifest['profile'] not in devices:
            raise DeckError('The offline driver does not match this Deck GPU and driver. Its caches will not be used.')
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        detail = getattr(error, 'stderr', None) or str(error)
        raise DeckError('This PC could not load the Deck driver. It may need a newer Linux C library. ' + detail[-1500:]) from error


def fetch_driver(deck, cache, note=print):
    if sys.platform != 'linux' or platform.machine() != 'x86_64':
        raise DeckError('Steam Deck shader preparation requires an x86-64 Linux PC.')
    try:
        probe = subprocess.run(ssh_command(deck, REMOTE_SNAPSHOT), capture_output=True, text=True, timeout=45, check=True)
        manifest = validate_manifest(json.loads(probe.stdout))
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        detail = getattr(error, 'stderr', None) or str(error)
        raise DeckError('Could not read the Deck driver. Configure SSH access first. ' + detail[-1500:]) from error
    key = hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()
    cache = Path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    bundle = cache / key
    try:
        recorded = json.loads((bundle / 'manifest.json').read_text())
    except (OSError, ValueError):
        recorded = None
    if recorded == manifest and (bundle / 'radv-deck.json').is_file() and \
            all((bundle / f['name']).is_file() and digest(bundle / f['name']) == f['sha256'] for f in manifest['files']):
        check_driver(bundle, manifest)
        return bundle
    note('Reading the Steam Deck driver for shader preparation on this PC')
    with tempfile.TemporaryDirectory(prefix='.deck-driver-', dir=cache) as temporary:
        staging = Path(temporary)
        for item in manifest['files']:
            path = staging / item['name']
            code = 'import sys; sys.stdout.buffer.write(open(%r, "rb").read())' % ('/usr/lib/' + item['name'])
            with path.open('wb') as stream:
                subprocess.run(ssh_command(deck, code), stdout=stream, stderr=subprocess.PIPE, timeout=120, check=True)
            if path.stat().st_size != item['size'] or digest(path) != item['sha256']:
                raise DeckError('The Deck driver changed while it was copied. Try again: ' + item['name'])
        (staging / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
        # The loader uses the final cache path, never the temporary download path.
        (staging / 'radv-deck.json').write_text(json.dumps({'file_format_version': '1.0.0', 'ICD': {
            'api_version': '1.3.0', 'library_path': str((bundle / 'libvulkan_radeon.so').resolve())}}) + '\n')
        if bundle.exists():
            shutil.rmtree(bundle)
        try:
            staging.rename(bundle)
        except FileExistsError:
            # Another builder can finish the same immutable snapshot first.
            if not all((bundle / f['name']).is_file() and digest(bundle / f['name']) == f['sha256']
                       for f in manifest['files']):
                raise DeckError('Another Steam Deck driver download is incomplete. Try again.')
    check_driver(bundle, manifest)
    return bundle


def worker_command(command, folder, env, limit):
    runner = shutil.which('systemd-run')
    if not runner:
        raise DeckError('Steam Deck preparation needs a systemd user session to enforce its memory limit.')
    args = [runner, '--user', '--quiet', '--wait', '--collect', '--pipe', '--expand-environment=no',
            '--unit=hookshot-deck-' + uuid.uuid4().hex, '--property=MemoryMax=' + str(limit),
            '--property=MemorySwapMax=0', '--property=OOMPolicy=kill', '--property=Nice=10',
            '--property=TimeoutStopSec=5s',
            '--working-directory=' + str(folder)]
    # Only the compiler environment goes into the worker, never unrelated account credentials.
    assignments = []
    for name, value in env.items():
        if name in ('PATH', 'HOME', 'LANG', 'LC_ALL', 'LD_PRELOAD', 'LD_LIBRARY_PATH', 'XDG_CACHE_HOME',
                    'AMDGPU_GPU_ID', 'MESA_SHADER_CACHE_DISABLE', 'MESA_SHADER_CACHE_DIR') or \
                name.startswith(('HOS_', 'RX_', 'HOOKSHOT_', 'VK_', 'EDEN_', 'NVN_', 'MESA_DISK_CACHE_')):
            assignments.append(name + '=' + value)
    return args + ['/usr/bin/env', '-i', *assignments, *command]


def stop_worker(command):
    unit = next(arg.split('=', 1)[1] for arg in command if arg.startswith('--unit='))
    subprocess.run(['systemctl', '--user', 'stop', unit], stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, timeout=30, check=False)


def memory_limit():
    available = None
    try:
        for line in Path('/proc/meminfo').read_text().splitlines():
            if line.startswith('MemAvailable:'):
                available = int(line.split()[1]) * 1024
    except (OSError, ValueError):
        pass
    if available is None or available < 8 * 1024**3:
        raise DeckError('Free at least 8 GiB of RAM before preparing Steam Deck shaders.')
    return min(8 * 1024**3, available // 2)


def check_worker():
    """Verify the memory controller before a costly build or preparation begins."""
    limit = memory_limit()
    code = ("from pathlib import Path; import sys; "
            "cg=Path('/sys/fs/cgroup')/Path('/proc/self/cgroup').read_text().split('::',1)[1].strip().lstrip('/'); "
            "value=(cg/'memory.max').read_text().strip(); "
            "assert value!='max' and int(value)<=int(sys.argv[1]), 'memory limit unavailable'")
    command = worker_command([sys.executable, '-I', '-c', code, str(limit)], Path.cwd(), {}, limit)
    try:
        subprocess.run(command, capture_output=True, text=True, timeout=30, check=True)
    except (OSError, subprocess.SubprocessError) as error:
        raise DeckError('A memory-limited systemd user worker could not be started. ' +
                        (getattr(error, 'stderr', None) or str(error))[-1000:]) from error
    finally:
        stop_worker(command)
    return limit


def copy_private(source, destination):
    """A COW snapshot where supported; a bounded-memory copy on other filesystems."""
    with Path(source).open('rb') as src, Path(destination).open('xb') as dst:
        try:
            import fcntl
            fcntl.ioctl(dst.fileno(), 0x40049409, src.fileno())  # Linux FICLONE, <linux/fs.h>.
        except (ImportError, OSError):
            shutil.copyfileobj(src, dst, 1024 * 1024)
    shutil.copystat(source, destination)


CACHE_FILES = ('hookshot_programs.bin', 'hookshot_programs.idx', 'hookshot_pipelines.bin',
               'hookshot_pipelines.bin.predicted', 'mkw_vk_pipelines.bin', 'hookshot_prepared.bin')
PREPARE_MARKERS = ('hookshot_prepared.bin.warming', 'hookshot_pipelines.bin.running',
                   'hookshot_pipelines.bin.predicting', 'hookshot_prepare_skipped')
DRIVER_CACHE = 'hookshot_steamdeck_cache'
PREPARE_BATCHES = 8


def prepare_game(game, bundle, workspace, runner=None):
    """Prepare in a disposable snapshot; publish caches only after verified success."""
    game, bundle, workspace = Path(game).resolve(), Path(bundle).resolve(), Path(workspace)
    limit = check_worker()
    workspace.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='deck-shaders-', dir=workspace) as temporary:
        staging = Path(temporary)
        executable_dir = game / 'bin' if (game / 'bin/hookshot').is_file() else game
        for name in ('hookshot', 'hookshot_vk.so'):
            copy_private(executable_dir / name, staging / name)
        inputs = ('hookshot_shaders.bin', 'hookshot_env.txt', 'hookshot_options.ini',
                  'hookshot_pipeline_states_shipped.bin', 'hookshot_pipeline_states.bin', 'hookshot_computes.bin')
        for name in inputs + CACHE_FILES:
            if (game / name).is_file():
                copy_private(game / name, staging / name)
        env = driver_environment(bundle)
        # Never read the PC's driver cache, including entries from a crashed preparation.
        # A successful run publishes this private disk cache with the game folder.
        reusable = False
        if (game / DRIVER_CACHE).is_dir():
            try:
                reusable = json.loads((game / 'hookshot_steamdeck_driver.json').read_text()) == \
                           json.loads((bundle / 'manifest.json').read_text())['profile']
            except (OSError, ValueError):
                pass
        if reusable:
            shutil.copytree(game / DRIVER_CACHE, staging / DRIVER_CACHE, copy_function=copy_private)
        else:
            (staging / DRIVER_CACHE).mkdir()
        env['MESA_SHADER_CACHE_DIR'] = str(staging / DRIVER_CACHE)
        env['MESA_SHADER_CACHE_DISABLE'] = 'false'
        print('Preparing Steam Deck shaders with at most 4 workers and %d GiB RAM' % (limit // 1024**3), flush=True)
        def cancelled(signum, frame):
            raise KeyboardInterrupt('Steam Deck preparation cancelled')
        # RADV can retain compiler allocations for a device's lifetime even when
        # its Vulkan pipelines are destroyed. Fresh processes bound that lifetime.
        for batch in range(PREPARE_BATCHES):
            env.update(HOOKSHOT_DECK_BATCH_COUNT=str(PREPARE_BATCHES), HOOKSHOT_DECK_BATCH_INDEX=str(batch))
            command = worker_command([str(staging / 'hookshot'), '--prepare-shaders'], staging, env, limit)
            previous = signal.signal(signal.SIGTERM, cancelled)
            try:
                code = runner(command, str(staging)) if runner else subprocess.call(command)
            finally:
                signal.signal(signal.SIGTERM, previous)
                stop_worker(command)
            if code != 0:
                raise DeckError('The preparation worker stopped (possibly its memory limit); installed caches were kept.')
            check_caches(staging, bundle)
        pending_cache = game / ('.deck-cache-' + uuid.uuid4().hex)
        previous_cache = game / ('.deck-cache-old-' + uuid.uuid4().hex)
        try:
            shutil.copytree(staging / DRIVER_CACHE, pending_cache, copy_function=copy_private)
            if (game / DRIVER_CACHE).exists():
                (game / DRIVER_CACHE).rename(previous_cache)
            try:
                pending_cache.rename(game / DRIVER_CACHE)
            except OSError:
                if previous_cache.exists():
                    previous_cache.rename(game / DRIVER_CACHE)
                raise
        finally:
            shutil.rmtree(pending_cache, ignore_errors=True)
            shutil.rmtree(previous_cache, ignore_errors=True)
        for name in CACHE_FILES + ('hookshot_steamdeck_driver.json',):
            if (staging / name).is_file():
                pending = game / (name + '.new')
                try:
                    pending.unlink(missing_ok=True)
                    copy_private(staging / name, pending)
                    os.replace(pending, game / name)
                finally:
                    pending.unlink(missing_ok=True)
        for name in PREPARE_MARKERS:
            (game / name).unlink(missing_ok=True)


def check_caches(game, bundle):
    """A successful prepare must leave records for this driver, not a false ready marker."""
    game, bundle = Path(game), Path(bundle)
    profile = json.loads((bundle / 'manifest.json').read_text())['profile']
    try:
        faults = game / 'hookshot_driver_faults.txt'
        if faults.is_file() and any(line.strip() and not line.startswith('#') for line in faults.read_text().splitlines()):
            raise ValueError('the offline driver reported compiler faults; incomplete caches were discarded')
        with (game / 'hookshot_programs.bin').open('rb') as stream:
            head = stream.read(16)
        if len(head) != 16 or head[:4] != b'HSP1':
            raise ValueError('missing shader cache header')
        stamp = struct.unpack_from('<Q', head, 8)[0]
        with (game / 'hookshot_prepared.bin').open('rb') as stream:
            prepared = stream.read(48)
        if len(prepared) != 48 or struct.unpack_from('<Q', prepared)[0] != stamp:
            raise ValueError('shader preparation did not finish')
        if struct.unpack_from('<Q', prepared, 16)[0] != (game / 'hookshot_shaders.bin').stat().st_size:
            raise ValueError('the prepared record belongs to a different shader list')
        if not struct.unpack_from('<Q', prepared, 40)[0]:
            raise ValueError('the driver preparation did not finish')
        path = game / 'mkw_vk_pipelines.bin'
        with path.open('rb') as stream:
            h = stream.read(32)
        if len(h) < 32:
            raise ValueError('incomplete pipeline cache')
        size, version, vendor, device = struct.unpack_from('<4I', h)
        if size < 32 or version != 1 or vendor != profile['vendor'] or device != profile['device'] or \
                h[16:32].hex() != profile['cache_uuid']:
            raise ValueError('pipeline cache belongs to another GPU or driver')
    except (OSError, ValueError, struct.error) as error:
        raise DeckError('Steam Deck shader preparation could not be verified: ' + str(error)) from error
    (game / 'hookshot_steamdeck_driver.json').write_text(json.dumps(profile, indent=2) + '\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--game', required=True, type=Path)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--deck', help='SSH destination, for example deck@steamdeck')
    source.add_argument('--driver', type=Path, help='an already fetched driver bundle (for local testing)')
    parser.add_argument('--cache', type=Path, default=Path(os.environ.get('XDG_CACHE_HOME') or Path.home() / '.cache') /
                        'hookshot-builder' / 'steamdeck-drivers')
    args = parser.parse_args()
    try:
        game = args.game.resolve()
        exe = game / 'bin' / 'hookshot' if (game / 'bin' / 'hookshot').is_file() else game / 'hookshot'
        if not exe.is_file() or not (game / 'hookshot_shaders.bin').is_file():
            raise DeckError('Choose a built Linux game folder with its shader list.')
        bundle = fetch_driver(args.deck, args.cache) if args.deck else args.driver
        manifest = validate_manifest(json.loads((bundle / 'manifest.json').read_text()))
        check_driver(bundle, manifest)
        prepare_game(game, bundle, args.cache / 'work')
    except (DeckError, OSError, subprocess.SubprocessError, ValueError) as error:
        print('Steam Deck: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
