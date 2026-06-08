"""
Test script to explore the Kconfig menu tree structure from ESP-IDF.
Helps debug the walk_node function in kconfig_dump.py.
"""
import json
import os
import sys
import tempfile

IDF_PATH = r"e:\espeim\.espressif\v5.5.4\esp-idf"
KCONFIG_ROOT = os.path.join(IDF_PATH, "Kconfig")
MAX_DEPTH = 3

def get_type_name(obj):
    """Safe type/class name."""
    return type(obj).__name__

def node_summary(node, depth=0):
    """Return a short summary string for a kconfiglib MenuNode."""
    parts = []
    parts.append(f"type={get_type_name(node)}")

    # is_menuconfig
    imc = getattr(node, 'is_menuconfig', None)
    parts.append(f"is_menuconfig={imc}")

    # prompt
    p = getattr(node, 'prompt', None)
    if p is not None:
        if isinstance(p, tuple) and len(p) > 0:
            parts.append(f'prompt="{p[0]}"')
        elif isinstance(p, str):
            parts.append(f'prompt="{p}"')
        else:
            parts.append(f'prompt={repr(p)[:80]}')

    # item (Symbol / Choice / None)
    item = getattr(node, 'item', None)
    if item is not None:
        iname = getattr(item, 'name', None) or ''
        itype = get_type_name(item)
        parts.append(f"item={itype}(name='{iname}')")
    else:
        parts.append("item=None")

    # children
    nodes_attr = getattr(node, 'nodes', None)
    list_attr = getattr(node, 'list', None)
    parts.append(f"nodes={type(nodes_attr).__name__ if nodes_attr is not None else 'None'}")
    parts.append(f"list={type(list_attr).__name__ if list_attr is not None else 'None'}")

    # count children
    child = nodes_attr if nodes_attr is not None else list_attr
    child_count = 0
    if child is not None:
        if isinstance(child, (list, tuple)):
            child_count = len(child)
        else:
            # attempt to walk as linked list
            cur = child
            try:
                while cur is not None:
                    child_count += 1
                    cur = getattr(cur, 'next', None)
            except:
                child_count = -1
    parts.append(f"children_count={child_count}")

    return "  " * depth + " | ".join(parts)


def print_tree(node, depth=0, label=""):
    """Print the menu tree up to MAX_DEPTH levels."""
    if depth > MAX_DEPTH:
        return

    prefix = "  " * depth
    if label:
        print(f"{prefix}[{label}]")
    print(node_summary(node, depth))

    # Get children
    child = getattr(node, 'nodes', None)
    if child is None:
        child = getattr(node, 'list', None)
    if child is None:
        if depth <= 1:
            print(f"{prefix}  (no children)")
        return

    # Convert to list
    if isinstance(child, (list, tuple)):
        siblings = child
    else:
        siblings = []
        cur = child
        while cur is not None:
            siblings.append(cur)
            cur = getattr(cur, 'next', None)

    count = len(siblings)
    if depth == 0:
        print(f"\nTotal children at root: {count}")
        print("=" * 80)

    for i, sib in enumerate(siblings):
        # Build a label for the child
        item = getattr(sib, 'item', None)
        iname = ""
        if item is not None:
            iname = getattr(item, 'name', None) or ''
        prom = getattr(sib, 'prompt', None)
        ptext = ""
        if prom is not None:
            if isinstance(prom, tuple) and len(prom) > 0:
                ptext = str(prom[0])
            elif isinstance(prom, str):
                ptext = prom
        imc = getattr(sib, 'is_menuconfig', False)
        label_text = f"#{i}"
        if imc:
            label_text += " MENUCONFIG"
        if ptext:
            label_text += f" '{ptext}'"
        if iname:
            label_text += f" [{iname}]"

        print_tree(sib, depth + 1, label_text)

        # Limit output: only print first 10 children at depth 0 (root level)
        if depth == 0 and i >= 9:
            print(f"{prefix}  ... ({count - 10} more children omitted)")
            break


def collect_stats(node, depth=0, stats=None):
    """Collect tree statistics."""
    if stats is None:
        stats = {"configs": 0, "menus": 0, "menuconfigs": 0, "choices": 0, "others": 0}

    if depth > MAX_DEPTH:
        return stats

    # Get children
    child = getattr(node, 'nodes', None)
    if child is None:
        child = getattr(node, 'list', None)
    if child is None:
        return stats

    if isinstance(child, (list, tuple)):
        siblings = child
    else:
        siblings = []
        cur = child
        while cur is not None:
            siblings.append(cur)
            cur = getattr(cur, 'next', None)

    for sib in siblings:
        imc = getattr(sib, 'is_menuconfig', False)
        item = getattr(sib, 'item', None)

        if imc:
            stats["menuconfigs"] += 1
        elif item is not None:
            itype = get_type_name(item)
            if 'Symbol' in itype:
                stats["configs"] += 1
            elif 'Choice' in itype:
                stats["choices"] += 1
            else:
                stats["others"] += 1
        else:
            # plain menu (no is_menuconfig, no item)
            stats["menus"] += 1

        # Recurse
        collect_stats(sib, depth + 1, stats)

    return stats


def main():
    print("=" * 80)
    print("Kconfig Tree Explorer")
    print("=" * 80)
    print(f"IDF_PATH: {IDF_PATH}")
    print(f"Kconfig root: {KCONFIG_ROOT}")
    print()

    if not os.path.isfile(KCONFIG_ROOT):
        print(f"ERROR: Kconfig root not found at {KCONFIG_ROOT}")
        sys.exit(1)

    # Set up environment for ESP-IDF Kconfig
    target = "esp32"
    sdkconfig_candidates = [
        os.path.join(r"e:\AIstdio\esp-ai-studio", "sdkconfig"),
    ]
    for cfg in sdkconfig_candidates:
        if os.path.isfile(cfg):
            with open(cfg, "r") as f:
                for line in f:
                    line = line.strip()
                    if line.startswith("CONFIG_IDF_TARGET="):
                        val = line.split("=", 1)[1].strip().strip('"')
                        if val:
                            target = val
                            break

    print(f"Detected target: {target}")

    # Create placeholder files for component Kconfig
    tmpdir = tempfile.mkdtemp(prefix="kconfig_test_")
    empty_file = os.path.join(tmpdir, "empty_kconfig")
    with open(empty_file, "w") as f:
        f.write("# Placeholder\n")

    os.environ.setdefault("IDF_PATH", IDF_PATH)
    os.environ.setdefault("IDF_TARGET", target)
    os.environ.setdefault("COMPONENT_KCONFIGS_SOURCE_FILE", empty_file)
    os.environ.setdefault("COMPONENT_KCONFIGS_PROJBUILD_SOURCE_FILE", empty_file)
    os.environ.setdefault("PROJECT_PATH", r"e:\AIstdio\esp-ai-studio")

    # Import kconfiglib
    sys.path.insert(0, os.path.join(IDF_PATH, "tools"))
    sys.path.insert(0, os.path.join(IDF_PATH, "tools", "kconfig_new"))
    try:
        import kconfiglib
    except ImportError as e:
        print(f"ERROR: Cannot import kconfiglib: {e}")
        sys.exit(1)

    print(f"kconfiglib imported from: {getattr(kconfiglib, '__file__', 'unknown')}")
    print(f"Kconfig class: {getattr(kconfiglib, 'Kconfig', 'NOT FOUND')}")

    # Parse Kconfig
    try:
        kconf = kconfiglib.Kconfig(KCONFIG_ROOT, warn_to_stderr=False)
    except Exception as e:
        print(f"ERROR parsing Kconfig: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)

    print(f"Kconfig parsed successfully.")
    print(f"Defined symbols: {len(kconf.unique_defined_syms)}")
    print()

    # Examine top_node
    top = kconf.top_node
    print(f"top_node type: {get_type_name(top)}")
    print(f"top_node attributes: {[a for a in dir(top) if not a.startswith('_')]}")
    print()

    # Check what .nodes returns
    tnodes = getattr(top, 'nodes', None)
    tlist = getattr(top, 'list', None)
    print(f"top_node.nodes type: {type(tnodes).__name__} value: {repr(tnodes)[:120]}")
    print(f"top_node.list type: {type(tlist).__name__} value: {repr(tlist)[:120]}")
    print()

    # Check if nodes is actually the head of a linked list
    if tnodes is not None and not isinstance(tnodes, (list, tuple)):
        # It's a single node - check if it has .next
        has_next = hasattr(tnodes, 'next')
        print(f"top_node.nodes is a single object. Has .next: {has_next}")
        if has_next:
            # Walk the linked list
            count = 0
            cur = tnodes
            first_items = []
            while cur is not None:
                count += 1
                if count <= 5:
                    it = getattr(cur, 'item', None)
                    if it is not None:
                        first_items.append(f"  {get_type_name(it)}(name='{getattr(it, 'name', '')}')")
                    else:
                        p = getattr(cur, 'prompt', None)
                        ptext = "?"
                        if p is not None:
                            if isinstance(p, tuple) and len(p) > 0:
                                ptext = str(p[0])
                            elif isinstance(p, str):
                                ptext = p
                        first_items.append(f"  MenuNode(prompt='{ptext}', is_menuconfig={getattr(cur, 'is_menuconfig', False)})")
                cur = getattr(cur, 'next', None)
            print(f"Linked list length: {count}")
            print(f"First 5 items:")
            for fi in first_items:
                print(fi)
        else:
            print("NOTE: top_node.nodes is a single node WITHOUT .next. Children count = 1")
    print()

    # Print the tree
    print("=" * 80)
    print(f"MENU TREE (first {MAX_DEPTH} levels)")
    print("=" * 80)
    print_tree(top)
    print()

    # Statistics
    print("=" * 80)
    print(f"STATISTICS (first {MAX_DEPTH} levels)")
    print("=" * 80)
    stats = collect_stats(top)
    print(f"  menuconfig items: {stats['menuconfigs']}")
    print(f"  config symbols:   {stats['configs']}")
    print(f"  choices:          {stats['choices']}")
    print(f"  plain menus:      {stats['menus']}")
    print(f"  other:            {stats['others']}")
    print(f"  TOTAL:            {sum(stats.values())}")

    # Extra: check what "Component config" menu looks like
    print()
    print("=" * 80)
    print("LOOKING FOR 'Component config' MENU")
    print("=" * 80)
    child = getattr(top, 'nodes', None) or getattr(top, 'list', None)
    if child is not None:
        if isinstance(child, (list, tuple)):
            siblings = child
        else:
            siblings = []
            cur = child
            while cur is not None:
                siblings.append(cur)
                cur = getattr(cur, 'next', None)

        for i, sib in enumerate(siblings):
            p = getattr(sib, 'prompt', None)
            ptext = ""
            if p is not None:
                if isinstance(p, tuple) and len(p) > 0:
                    ptext = str(p[0])
                elif isinstance(p, str):
                    ptext = p
            if "Component" in ptext or "component" in ptext.lower():
                print(f"Found at index {i}:")
                print(node_summary(sib, 0))
                # Check its children
                ch = getattr(sib, 'nodes', None) or getattr(sib, 'list', None)
                if ch is not None:
                    if isinstance(ch, (list, tuple)):
                        ch_count = len(ch)
                        print(f"  Children (list): {ch_count}")
                        for j, c in enumerate(ch):
                            if j < 5:
                                print(f"    {node_summary(c, 1)}")
                    else:
                        ccount = 0
                        curc = ch
                        while curc is not None:
                            ccount += 1
                            if ccount <= 5:
                                print(f"    {node_summary(curc, 1)}")
                            curc = getattr(curc, 'next', None)
                        print(f"  Children (linked list): {ccount}")
                else:
                    print("  NO CHILDREN (nodes/list is None)")
    print()

    # Also try checking kconf.root_menu or similar
    print("=" * 80)
    print("CHECKING kconfiglib internal structure")
    print("=" * 80)
    for attr in ['root_menu', 'menus', 'menuconfig_nodes', 'defined_syms', 'n/m/env_vars']:
        val = getattr(kconf, attr, None)
        if val is not None:
            print(f"  kconf.{attr}: type={get_type_name(val)}, len={getattr(val, '__len__', lambda: 'N/A')()}")
        else:
            print(f"  kconf.{attr}: None")


if __name__ == "__main__":
    main()