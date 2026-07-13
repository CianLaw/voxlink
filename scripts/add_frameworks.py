import sys, re, uuid

pbxproj_path = sys.argv[1]
with open(pbxproj_path, 'r') as f:
    content = f.read()

def gen_uuid():
    return uuid.uuid4().hex[:24].upper()

frameworks = ["AudioToolbox", "AVFoundation", "Foundation", "CoreFoundation"]

added_file_refs = {}
added_build_files = {}

for fw in frameworks:
    fw_name = f"{fw}.framework"
    if f'name = {fw_name};' in content or f'path = System/Library/Frameworks/{fw_name}' in content:
        print(f"  {fw_name} already in project")
        continue
    fuuid = gen_uuid()
    while fuuid in content:
        fuuid = gen_uuid()
    file_ref = f'\t\t{fuuid} = {{isa = PBXFileReference; lastKnownFileType = wrapper.framework; name = {fw_name}; path = System/Library/Frameworks/{fw_name}; sourceTree = SDKROOT; }};'
    marker = "/* End PBXFileReference section */"
    if marker in content:
        content = content.replace(marker, f"{file_ref}\n{marker}", 1)
        added_file_refs[fw] = fuuid
        print(f"  Added PBXFileReference for {fw_name} ({fuuid})")

for fw, fuuid in added_file_refs.items():
    buuid = gen_uuid()
    while buuid in content:
        buuid = gen_uuid()
    build_file = f'\t\t{buuid} = {{isa = PBXBuildFile; fileRef = {fuuid}; }};'
    marker = "/* End PBXBuildFile section */"
    if marker in content:
        content = content.replace(marker, f"{build_file}\n{marker}", 1)
        added_build_files[fw] = buuid
        print(f"  Added PBXBuildFile for {fw} ({buuid})")

targets_start = content.find("/* Begin PBXNativeTarget section */")
targets_end = content.find("/* End PBXNativeTarget section */")
frameworks_phase_uuids = set()

if targets_start >= 0 and targets_end >= 0:
    targets_section = content[targets_start:targets_end]
    for m in re.finditer(r'(\w{24})\s*/\*\s*Frameworks\s*\*/', targets_section):
        frameworks_phase_uuids.add(m.group(1))
        print(f"  Found Frameworks phase UUID: {m.group(1)}")

print(f"  Total {len(frameworks_phase_uuids)} Frameworks build phases")

for phase_uuid in frameworks_phase_uuids:
    pattern = re.compile(re.escape(phase_uuid) + r'\s*/\*[^*]*\*/\s*=\s*\{')
    defn_match = pattern.search(content)
    if not defn_match:
        pattern2 = re.compile(re.escape(phase_uuid) + r'\s*=\s*\{')
        defn_match = pattern2.search(content)
    if not defn_match:
        print(f"  WARNING: phase definition for {phase_uuid} not found")
        continue
    
    brace_start = defn_match.end() - 1
    
    brace_count = 0
    files_paren_close = -1
    in_files = False
    paren_count = 0
    files_equals_pos = -1
    
    i = brace_start
    while i < len(content):
        c = content[i]
        if c == '{':
            brace_count += 1
        elif c == '}':
            brace_count -= 1
            if brace_count == 0:
                break
        elif not in_files:
            if c == '(':
                before = content[brace_start:i]
                if re.search(r'files\s*=\s*$', before.rstrip()):
                    in_files = True
                    paren_count = 1
        elif in_files:
            if c == '(':
                paren_count += 1
            elif c == ')':
                paren_count -= 1
                if paren_count == 0:
                    files_paren_close = i
                    in_files = False
        i += 1
    
    if files_paren_close < 0:
        print(f"  WARNING: Could not find files() closing paren in phase {phase_uuid}")
        continue
    
    insert_pos = files_paren_close
    for j in range(files_paren_close - 1, brace_start, -1):
        if content[j] in ' \t\n\r':
            insert_pos = j
        else:
            break
    
    additions = ""
    for fw, buuid in added_build_files.items():
        if buuid not in content[content.find('files', brace_start):files_paren_close]:
            additions += f'\t\t\t\t{buuid} /* {fw}.framework in Frameworks */,\n'
    
    if not additions:
        print(f"  Phase {phase_uuid}: nothing to add")
        continue
    
    indent = "\n\t\t\t"
    content = content[:insert_pos] + indent + additions + content[insert_pos:]
    print(f"  Phase {phase_uuid}: added {len(added_build_files)} frameworks")

with open(pbxproj_path, 'w') as f:
    f.write(content)

print(f"Done. Added frameworks: {list(added_file_refs.keys())}")
