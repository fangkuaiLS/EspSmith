# Kconfig parser - outputs menu tree as JSON for the SDK Config Editor.
# Uses kconfiglib (bundled with ESP-IDF) to parse Kconfig without CMake/ninja.
import json, sys, os, tempfile

if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8')
elif hasattr(sys.stdout, 'buffer'):
    sys.stdout = open(sys.stdout.fileno(), mode='w', encoding='utf-8', buffering=1, closefd=False)

def main():
    if len(sys.argv) < 3:
        print(json.dumps({"error": "Usage: kconfig_dump.py <idf_path> <project_path> [sdkconfig_path]"}))
        sys.exit(1)

    idf_path = sys.argv[1]
    project_path = sys.argv[2]
    sdkconfig_path = sys.argv[3] if len(sys.argv) > 3 else os.path.join(project_path, "sdkconfig")

    target = "esp32"
    for cfg_file in [sdkconfig_path, os.path.join(project_path, "sdkconfig.defaults")]:
        if os.path.isfile(cfg_file):
            try:
                with open(cfg_file, "r") as f:
                    for line in f:
                        line = line.strip()
                        if line.startswith("CONFIG_IDF_TARGET="):
                            val = line.split("=", 1)[1].strip().strip('"')
                            if val: target = val
            except: pass

    tmpdir = tempfile.mkdtemp(prefix="kconfig_")

    def find_component_kconfigs(base_dir):
        result, projbuild_result = [], []
        if not os.path.isdir(base_dir): return result, projbuild_result
        for entry in sorted(os.listdir(base_dir)):
            comp_dir = os.path.join(base_dir, entry)
            if not os.path.isdir(comp_dir): continue
            kconfig = os.path.join(comp_dir, "Kconfig")
            if os.path.isfile(kconfig): result.append(kconfig)
            projbuild = os.path.join(comp_dir, "Kconfig.projbuild")
            if os.path.isfile(projbuild): projbuild_result.append(projbuild)
        return result, projbuild_result

    idf_comp_dir = os.path.join(idf_path, "components")
    idf_kconfigs, idf_projbuilds = find_component_kconfigs(idf_comp_dir)
    proj_comp_dir = os.path.join(project_path, "components")
    proj_kconfigs, proj_projbuilds = find_component_kconfigs(proj_comp_dir)
    main_projbuild = os.path.join(project_path, "main", "Kconfig.projbuild")
    main_kconfig = os.path.join(project_path, "main", "Kconfig")

    comp_kconfigs_file = os.path.join(tmpdir, "component_kconfigs")
    with open(comp_kconfigs_file, "w") as f:
        f.write("# Auto-generated component Kconfig sources\n")
        for k in idf_kconfigs: f.write('source "{}"\n'.format(k.replace("\\", "/")))
        for k in proj_kconfigs: f.write('source "{}"\n'.format(k.replace("\\", "/")))
        if os.path.isfile(main_kconfig): f.write('source "{}"\n'.format(main_kconfig.replace("\\", "/")))

    comp_projbuild_file = os.path.join(tmpdir, "component_kconfigs_projbuild")
    with open(comp_projbuild_file, "w") as f:
        f.write("# Auto-generated projbuild Kconfig sources\n")
        for k in idf_projbuilds: f.write('source "{}"\n'.format(k.replace("\\", "/")))
        for k in proj_projbuilds: f.write('source "{}"\n'.format(k.replace("\\", "/")))
        if os.path.isfile(main_projbuild): f.write('source "{}"\n'.format(main_projbuild.replace("\\", "/")))

    os.environ.setdefault("IDF_PATH", idf_path)
    os.environ.setdefault("IDF_TARGET", target)
    os.environ["COMPONENT_KCONFIGS_SOURCE_FILE"] = comp_kconfigs_file
    os.environ["COMPONENT_KCONFIGS_PROJBUILD_SOURCE_FILE"] = comp_projbuild_file
    os.environ.setdefault("PROJECT_PATH", project_path)

    sys.path.insert(0, os.path.join(idf_path, "tools"))
    sys.path.insert(0, os.path.join(idf_path, "tools", "kconfig_new"))
    try: import kconfiglib
    except ImportError:
        print(json.dumps({"error": "kconfiglib not found"})); sys.exit(1)

    _Symbol = getattr(kconfiglib, 'Symbol', None)
    _Choice = getattr(kconfiglib, 'Choice', None)
    _BOOL=getattr(kconfiglib,'BOOL',None); _TRISTATE=getattr(kconfiglib,'TRISTATE',None)
    _INT=getattr(kconfiglib,'INT',None); _HEX=getattr(kconfiglib,'HEX',None)
    _STRING=getattr(kconfiglib,'STRING',None); _MenuNode=getattr(kconfiglib,'MenuNode',None)

    if _Symbol is None:
        try:
            from kconfiglib.kconfiglib import Symbol as _Symbol, Choice as _Choice
            from kconfiglib.kconfiglib import BOOL as _BOOL, TRISTATE as _TRISTATE
            from kconfiglib.kconfiglib import INT as _INT, HEX as _HEX, STRING as _STRING, MenuNode as _MenuNode
        except: pass
    if _Symbol is None:
        try:
            from kconfiglib.core import Symbol as _Symbol, Choice as _Choice
            from kconfiglib.core import BOOL as _BOOL, TRISTATE as _TRISTATE
            from kconfiglib.core import INT as _INT, HEX as _HEX, STRING as _STRING, MenuNode as _MenuNode
        except: pass

    if _MenuNode is not None:
        _orig = _MenuNode.__init__
        def _patched(self,*a,**kw):
            _orig(self,*a,**kw)
            try: self.help
            except AttributeError: self.help=None
            try: self.ranges
            except AttributeError: self.ranges=None
        _MenuNode.__init__=_patched

    kconfig_root = os.path.join(idf_path, "Kconfig")
    if not os.path.isfile(kconfig_root):
        print(json.dumps({"error": f"Kconfig root not found at {kconfig_root}"})); sys.exit(1)

    kconf = kconfiglib.Kconfig(kconfig_root, warn_to_stderr=False)
    if _Symbol is None:
        for s in kconf.unique_defined_syms: _Symbol=type(s); break
    if _Choice is None:
        for c in getattr(kconf,'unique_choices',[]): _Choice=type(c); break
    if _Symbol is None: _Symbol=type('_S',(),{})
    if _Choice is None: _Choice=type('_C',(),{})
    if _BOOL is None: _BOOL=0
    if _TRISTATE is None: _TRISTATE=1
    if _INT is None: _INT=2
    if _HEX is None: _HEX=3
    if _STRING is None: _STRING=4

    if os.path.isfile(sdkconfig_path):
        try: kconf.load_config(sdkconfig_path, replace=True)
        except: pass

    # Pass 1: kconfiglib resolved values
    values = {}
    for sym in kconf.unique_defined_syms:
        values[sym.name] = sym.str_value

    # Pass 2: override with FILE values (authoritative)
    _pass2_overrides = 0
    if os.path.isfile(sdkconfig_path):
        try:
            with open(sdkconfig_path, "r", encoding="utf-8", errors="replace") as f:
                for line in f:
                    line = line.strip()
                    if not line: continue
                    # Handle "# CONFIG_XXX is not set" (ESP-IDF's =n format)
                    if line.startswith("# CONFIG_") and line.endswith(" is not set"):
                        key = line[2:-11].strip()  # strip "# " prefix and " is not set" suffix
                        sym_key = key[7:] if key.startswith("CONFIG_") else key
                        if sym_key in values:
                            if values[sym_key] != 'n': _pass2_overrides += 1
                            values[sym_key] = 'n'
                        continue
                    # Skip regular comments
                    if line.startswith("#"): continue
                    eq_pos = line.find("=")
                    if eq_pos < 0: continue
                    key = line[:eq_pos].strip()
                    val = line[eq_pos+1:].strip()
                    sym_key = key[7:] if key.startswith("CONFIG_") else key
                    if sym_key in values:
                        if values[sym_key] != val: _pass2_overrides += 1
                        values[sym_key] = val
        except Exception as e:
            print(json.dumps({"error": f"Pass 2 failed: {e}"})); sys.exit(1)

    # Pass 3: push file values BACK into kconfiglib symbols
    _pass3_count = 0
    for sym in kconf.unique_defined_syms:
        if sym.name in values:
            try:
                sym.set_value(values[sym.name])
                _pass3_count += 1
            except: pass

    # Build verify sample AFTER all passes
    _verify_keys = [k for k in values if any(x in k for x in ['BOARD_TYPE','BUILD_TYPE','SCREEN_TYPE','DISPLAY','LCD','SPIFFS'])][:10]
    _verify_sample = [(k, values[k]) for k in _verify_keys]
    print(f"[VERIFY] pass2_overrides={_pass2_overrides} pass3_set={_pass3_count}", file=sys.stderr)
    print(f"[VERIFY] sample: {json.dumps(_verify_sample)}", file=sys.stderr)

    def get_prompt_text(obj):
        p = getattr(obj, 'prompt', None)
        if p is not None:
            if isinstance(p, tuple) and len(p)>0: return p[0]
            if isinstance(p, str): return p; return str(p)
        ps = getattr(obj, 'prompts', None)
        if ps and len(ps)>0:
            p0 = ps[0]
            if isinstance(p0, tuple) and len(p0)>0: return p0[0]
            return str(p0)
        return None

    def get_help_text(obj):
        h = getattr(obj, 'help', None)
        if h is not None: return str(h)
        nodes = getattr(obj, 'nodes', None)
        if nodes:
            hn = getattr(nodes, 'help', None)
            if hn is not None: return str(hn)
        return ""

    def walk_node(node):
        items = []
        child = getattr(node, 'nodes', None)
        if child is None: child = getattr(node, 'list', None)
        if child is None: return items
        if isinstance(child, (list, tuple)): siblings = child
        else:
            siblings = []; cur = child
            while cur is not None:
                siblings.append(cur); cur = getattr(cur, 'next', None)
        for item in siblings:
            menu_item = getattr(item, 'item', None)
            is_choice = False
            if menu_item is not None and hasattr(menu_item, 'syms') and hasattr(menu_item, 'str_value'):
                syms = getattr(menu_item, 'syms', None)
                if syms and hasattr(syms, '__iter__'): is_choice = True
            if is_choice: items.append(choice_to_json(menu_item))
            elif getattr(item, 'is_menuconfig', False):
                sub = walk_node(item)
                items.append({"type":"menu","title":get_prompt_text(item) or "","help":get_help_text(item),"items":sub})
            elif isinstance(menu_item, _Symbol) and menu_item.name:
                items.append(config_to_json(menu_item))
            else:
                kid = getattr(item, 'nodes', None)
                hk = len(kid)>0 if isinstance(kid,(list,tuple)) else (kid is not None)
                if hk:
                    sub = walk_node(item)
                    if sub: items.append({"type":"menu","title":get_prompt_text(item) or "","help":get_help_text(item),"items":sub})
        return items

    def config_to_json(sym):
        name = sym.name
        val = values.get(name, sym.str_value) if name else sym.str_value
        e = {"type":"config","name":name,"title":get_prompt_text(sym) or name,"help":get_help_text(sym),"value":val}
        if sym.type in (_BOOL, _TRISTATE): e["typeHint"]="bool"
        elif sym.type==_INT:
            e["typeHint"]="int"
            try:
                rng=getattr(sym,'ranges',None)
                if rng: e["range"]=[rng[0][0],rng[0][1]]
            except: pass
        elif sym.type==_HEX: e["typeHint"]="hex"
        elif sym.type==_STRING: e["typeHint"]="string"
        return e

    def choice_to_json(ch):
        choices = []; selected_name = None
        for sym in ch.syms:
            sv = values.get(sym.name, sym.str_value) if sym.name else sym.str_value
            is_sel = sv == 'y'
            choices.append({"name":sym.name,"title":get_prompt_text(sym) or sym.name,"value":sv})
            if is_sel: selected_name = sym.name
        return {"type":"choice","name":ch.name if hasattr(ch,'name') else None,
                "title":get_prompt_text(ch) or (ch.name if hasattr(ch,'name') else ""),
                "help":get_help_text(ch),"value":selected_name,"typeHint":"choice","choices":choices}

    menu_tree = walk_node(kconf.top_node)
    menu_names = set()
    def cn(items):
        for it in items:
            if "name" in it: menu_names.add(it["name"])
            if "items" in it: cn(it["items"])
    cn(menu_tree)
    orphans = []
    for sym in kconf.unique_defined_syms:
        if sym.name and sym.name not in menu_names: orphans.append(config_to_json(sym))
    if orphans: menu_tree.append({"type":"menu","title":"Other Components","help":"","items":orphans})

    result = {
        "version":1,"values":values,"menus":menu_tree,"sdkconfigPath":sdkconfig_path,
        "_diag":{"pass2_overrides":_pass2_overrides,"pass3_set":_pass3_count,"verify_sample":_verify_sample},
    }
    print(json.dumps(result, ensure_ascii=False, default=str))

if __name__ == "__main__": main()
