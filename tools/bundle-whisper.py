#!/usr/bin/env python3
"""Bundle the locally installed whisper.cpp and its Homebrew dependencies on macOS."""
import pathlib
import shutil
import subprocess
import sys

bundle = pathlib.Path(sys.argv[1]).resolve()
macos = bundle / 'Contents' / 'MacOS'
frameworks = bundle / 'Contents' / 'Frameworks'
licenses = bundle / 'Contents' / 'Resources' / 'Licenses'
frameworks.mkdir(parents=True, exist_ok=True)
licenses.mkdir(parents=True, exist_ok=True)

def run(*args):
    return subprocess.check_output(args, text=True).strip()

def prefix(formula):
    brew = shutil.which('brew')
    if not brew:
        raise RuntimeError('Homebrew is required to build the bundled speech engine')
    return (pathlib.Path(brew).parent.parent / 'opt' / formula).resolve(strict=True)

whisper, ggml, omp = (prefix(x) for x in ('whisper-cpp', 'ggml', 'libomp'))
search = [whisper / 'lib', ggml / 'lib', omp / 'lib']
for source, name in ((whisper / 'LICENSE', 'whisper.cpp.txt'), (ggml / 'LICENSE', 'ggml.txt'), (omp / 'LICENSE.TXT', 'libomp.txt')):
    shutil.copy2(source, licenses / name)

queue = []
copied = {}
def add(source, destination):
    source = source.resolve(strict=True)
    if destination in copied:
        if copied[destination] != source:
            raise RuntimeError(f'Conflicting library names: {source}, {copied[destination]}')
        return
    shutil.copy2(source, destination)
    destination.chmod(destination.stat().st_mode | 0o200)
    copied[destination] = source
    queue.append(destination)

for name in ('whisper-cli', 'whisper-server'):
    add(whisper / 'bin' / name, macos / name)
plugins = list((ggml / 'libexec').glob('libggml-*.so'))
if not plugins:
    raise RuntimeError('No ggml backend plugins found')
for plugin in plugins:
    add(plugin, macos / plugin.name)

while queue:
    target = queue.pop(0)
    lines = run('otool', '-L', str(target)).splitlines()[1:]
    for line in lines:
        dependency = line.strip().split(' (compatibility version')[0]
        if dependency.startswith(('/usr/lib/', '/System/Library/')):
            continue
        name = pathlib.Path(dependency).name
        # A dylib lists its own install name first; it is not a dependency.
        if name == target.name and target.suffix == '.dylib':
            continue
        candidate = pathlib.Path(dependency)
        if not candidate.is_absolute() or not candidate.is_file():
            candidate = next((d / name for d in search if (d / name).is_file()), None)
        if candidate is None or not candidate.is_file():
            raise RuntimeError(f'Cannot resolve {dependency} required by {target}')
        add(candidate, frameworks / name)
        subprocess.check_call(['install_name_tool', '-change', dependency, '@executable_path/../Frameworks/' + name, str(target)])
    if target.suffix == '.dylib':
        subprocess.check_call(['install_name_tool', '-id', '@rpath/' + target.name, str(target)])
# Homebrew bakes its own backend directory into libggml. Point that search at
# an absent location so ggml falls back to the directory beside whisper-cli.
# Fail closed if the upstream build changes rather than ship a hidden dependency.
ggml_binary = frameworks / 'libggml.0.dylib'
data = ggml_binary.read_bytes()
embedded = str(ggml / 'libexec').encode()
if data.count(embedded + b'\0') != 1:
    raise RuntimeError('Unexpected ggml backend path; cannot make the build portable')
replacement = b'/nonexistent' + b'\0' * (len(embedded) - len(b'/nonexistent'))
ggml_binary.write_bytes(data.replace(embedded, replacement, 1))
print(f'Bundled whisper.cpp and {len(copied) - 2} libraries/backends')
