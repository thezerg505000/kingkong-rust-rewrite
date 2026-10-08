// ApplyPS2Names: name functions in the PS2 ELF (SLUS_213.11) from the recovered AI2C tables.
// Usage (headless): analyzeHeadless <proj> KKPS2 -process SLUS_213.11 -noanalysis
//                   -scriptPath <dir> -postScript ApplyPS2Names.java <namesDir> [outTsv]
// namesDir holds ai2c_functions_ps2.json and ai2c_triggers_ps2.json (written by ai2c_names_ps2.py);
// extra_names_ps2.json ({"0xaddr":"Name"}) is applied too if present.
// Creates functions where none exist (disassembling first if the bytes are not code), renames
// FUN_/thunk_FUN_ ones, adds extra names as labels. Names shared by >6 entries get AI2C_shared_stub_<addr>.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import com.google.gson.*;
import java.io.*;
import java.util.*;

public class ApplyPS2Names extends GhidraScript {
    Map<Long, List<String>> names = new LinkedHashMap<>();

    void addName(long addr, String name) {
        List<String> l = names.computeIfAbsent(addr, k -> new ArrayList<>());
        if (!l.contains(name)) l.add(name);
    }

    void loadJson(File f) throws Exception {
        if (!f.exists()) { println("missing " + f); return; }
        JsonElement root = JsonParser.parseReader(new FileReader(f));
        if (root.isJsonArray()) {
            for (JsonElement e : root.getAsJsonArray()) {
                JsonObject o = e.getAsJsonObject();
                addName(Long.decode(o.get("addr").getAsString()), o.get("name").getAsString());
            }
        } else {
            for (Map.Entry<String, JsonElement> e : root.getAsJsonObject().entrySet())
                addName(Long.decode(e.getKey()), e.getValue().getAsString());
        }
    }

    static String safe(String s) { return s.replaceAll("[^A-Za-z0-9_]", "_"); }

    public void run() throws Exception {
        String[] a = getScriptArgs();
        File dir = new File(a[0]);
        loadJson(new File(dir, "ai2c_functions_ps2.json"));
        loadJson(new File(dir, "ai2c_triggers_ps2.json"));
        loadJson(new File(dir, "extra_names_ps2.json"));
        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolTable st = currentProgram.getSymbolTable();
        AddressSpace sp = currentProgram.getAddressFactory().getDefaultAddressSpace();
        int renamed = 0, created = 0, failed = 0, kept = 0;
        PrintWriter tsv = a.length > 1 ? new PrintWriter(new File(a[1])) : null;
        if (tsv != null) tsv.println("addr\tname\taliases\tstatus");
        for (Map.Entry<Long, List<String>> e : names.entrySet()) {
            Address addr = sp.getAddress(e.getKey());
            List<String> l = e.getValue();
            String primary = l.size() > 6 ? "AI2C_shared_stub_" + Long.toHexString(e.getKey()) : safe(l.get(0));
            String status = "ok";
            Function f = fm.getFunctionAt(addr);
            if (f == null) {
                if (getInstructionAt(addr) == null) {
                    disassemble(addr);
                }
                f = createFunction(addr, primary);
                if (f == null) {
                    failed++;
                    status = "NOFUNC";
                    if (tsv != null) tsv.println(String.format("%08x\t%s\t%s\t%s", e.getKey(), primary, String.join("|", l), status));
                    continue;
                }
                created++;
                status = "created";
            }
            if (f.getName().startsWith("FUN_") || f.getName().startsWith("thunk_FUN_") || f.getName().startsWith("AI2C_")) {
                try { f.setName(primary, SourceType.USER_DEFINED); renamed++; }
                catch (Exception ex) { println("rename fail " + primary + " " + ex); status = "RENAMEFAIL"; }
            } else kept++;
            for (int i = 0; i < l.size() && i < 40; i++) {
                String n = safe(l.get(i));
                if (!n.equals(f.getName())) {
                    try { st.createLabel(addr, n, SourceType.USER_DEFINED); } catch (Exception ex) {}
                }
            }
            if (tsv != null) tsv.println(String.format("%08x\t%s\t%s\t%s", e.getKey(), f.getName(), String.join("|", l), status));
        }
        if (tsv != null) tsv.close();
        println("APPLYPS2NAMES entries=" + names.size() + " renamed=" + renamed + " created=" + created + " kept=" + kept + " failed=" + failed);
    }
}
