import sys, re, uuid

pbxproj_path = sys.argv[1]
with open(pbxproj_path, 'r') as f:
    content = f.read()

def gen_uuid():
    return uuid.uuid4().hex[:24].upper()

frameworks_to_add = ["AudioToolbox", "AVFoundation", "Foundation", "CoreFoundation"]
added = []

for fw in frameworks_to_add:
    fw_name = f"{fw}.framework"
    if fw_name in content:
        print(f"  {fw_name} already exists, skipping")
        continue
    file_ref_uuid = gen_uuid()
    build_file_uuid = gen_uuid()
    file_ref = f'\t\t{file_ref_uuid} = {{isa = PBXFileReference; lastKnownFileType = wrapper.framework; name = {fw_name}; path = System/Library/Frameworks/{fw_name}; sourceTree = SDKROOT; }};'
    build_file = f'\t\t{build_file_uuid} = {{isa = PBXBuildFile; fileRef = {file_ref_uuid}; }};'
    marker1 = "/* End PBXFileReference section */"
    if marker1 in content:
        content = content.replace(marker1, f"{file_ref}\n{marker1}")
    else:
        print(f"  WARNING: no PBXFileReference end marker for {fw}")
        continue
    marker2 = "/* End PBXBuildFile section */"
    if marker2 in content:
        content = content.replace(marker2, f"{build_file}\n{marker2}")
    phase_match = re.search(r'(\w{24})\s*=\s*\{\s*isa\s*=\s*PBXFrameworksBuildPhase;', content)
    if phase_match:
        phase_uuid = phase_match.group(1)
        pat = re.compile(r'(' + re.escape(phase_uuid) + r'\s*=\s*\{[^}]*?files\s*=\s*\()([^)]*?)(\)\s*;)', re.DOTALL)
        def repl(m):
            items = m.group(2).rstrip()
            if items and not items.rstrip().endswith(','):
                items += ','
            return f'{m.group(1)}\n\t\t\t\t{build_file_uuid} /* {fw_name} in Frameworks */,{m.group(3)}'
        content, n = pat.subn(repl, content, count=1)
        if n == 0:
            print(f"  WARNING: could not add {fw} to frameworks phase")
            continue
    added.append(fw)
    print(f"  Added {fw_name}")

with open(pbxproj_path, 'w') as f:
    f.write(content)
print(f"Done. Added: {added}")
