import sys, collections, re
pid = sys.argv[1]
by = collections.Counter(); usb = collections.Counter()
name = None
for line in open(f'/proc/{pid}/smaps'):
    if re.match(r'^[0-9a-f]+-[0-9a-f]+ ', line):
        parts = line.split(None, 5)
        name = parts[5].strip() if len(parts) > 5 else '[anon]'
        name = re.sub(r'.*/', '', name) or '[anon]'
    elif line.startswith('Pss:'):
        by[name] += int(line.split()[1])
    elif line.startswith('Private_Clean:') or line.startswith('Private_Dirty:'):
        usb[name] += int(line.split()[1])
tot = sum(by.values())
print(f'total PSS {tot/1024:.1f} MiB')
for k, v in by.most_common(14):
    print(f'{v/1024:7.1f} MiB pss  {usb[k]/1024:7.1f} MiB uss  {k}')
