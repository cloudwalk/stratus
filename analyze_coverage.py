import json

with open('./target/llvm-cov/coverage.json', 'r') as f:
    data = json.load(f)
    for file in data['files']:
        if 'primitives/hash.rs' in file['filename']:
            print(f'Coverage for {file["filename"]}:')
            print(f'Lines covered: {file["summary"]["lines"]["covered"]}')
            print(f'Lines uncovered: {file["summary"]["lines"]["uncovered"]}')
            print('\nUncovered lines:')
            for region in file['regions']:
                if region['count'] == 0:
                    print(f'Line {region["line"]}: {region["name"] if "name" in region else "unnamed region"}')
