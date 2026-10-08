// KKExport: apply recovered names to KingKong8.exe and export a greppable knowledge base.
// Set KK_EXPORT_RESUME=1 to reuse already exported functions/*.c (decompiling takes ~1 h).
// Usage (headless): analyzeHeadless <proj> KK8 -process KingKong8.exe -noanalysis
//                   -scriptPath <dir> -postScript KKExport.java <namesDir> <outDir>
// namesDir holds ai2c_functions.json, ai2c_triggers.json, extra_names.json.
// outDir receives functions/<name>_<addr>.c, index/functions.jsonl, index/strings.tsv,
// index/names.tsv, index/callgraph.tsv
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.data.*;
import ghidra.program.model.mem.*;
import ghidra.util.task.ConsoleTaskMonitor;
import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;

public class KKExport extends GhidraScript {
    Map<Long, List<String>> names = new HashMap<>();

    void addName(long addr, String name) {
        names.computeIfAbsent(addr, k -> new ArrayList<>());
        List<String> l = names.get(addr);
        if (!l.contains(name)) l.add(name);
    }

    void loadJson(File f, String addrKey, String nameKey) throws Exception {
        if (!f.exists()) { println("missing " + f); return; }
        JsonElement root = JsonParser.parseReader(new FileReader(f));
        if (root.isJsonArray()) {
            for (JsonElement e : root.getAsJsonArray()) {
                JsonObject o = e.getAsJsonObject();
                addName(Long.decode(o.get(addrKey).getAsString()), o.get(nameKey).getAsString());
            }
        } else {
            for (Map.Entry<String, JsonElement> e : root.getAsJsonObject().entrySet()) {
                addName(Long.decode(e.getKey()), e.getValue().getAsString());
            }
        }
    }

    static String safe(String s) { return s.replaceAll("[^A-Za-z0-9_]", "_"); }

    public void run() throws Exception {
        String[] a = getScriptArgs();
        File namesDir = new File(a[0]);
        File out = new File(a[1]);
        new File(out, "functions").mkdirs();
        new File(out, "index").mkdirs();
        loadJson(new File(namesDir, "ai2c_functions.json"), "addr", "name");
        loadJson(new File(namesDir, "ai2c_triggers.json"), "addr", "name");
        loadJson(new File(namesDir, "extra_names.json"), "", "");

        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolTable st = currentProgram.getSymbolTable();
        AddressFactory af = currentProgram.getAddressFactory();
        int renamed = 0, created = 0;
        PrintWriter namesOut = new PrintWriter(new File(out, "index/names.tsv"));
        namesOut.println("addr\tprimary\taliases\tsource");
        for (Map.Entry<Long, List<String>> e : names.entrySet()) {
            Address addr = af.getDefaultAddressSpace().getAddress(e.getKey());
            List<String> l = e.getValue();
            String primary;
            // generic stubs shared by many AI2C entries keep a neutral name
            if (l.size() > 6) primary = "AI2C_shared_stub_" + Long.toHexString(e.getKey());
            else primary = safe(l.get(0));
            Function f = fm.getFunctionAt(addr);
            if (f == null) {
                f = createFunction(addr, primary);
                if (f == null) { namesOut.println(String.format("%08x\t%s\t%s\tNOFUNC", e.getKey(), primary, String.join("|", l))); continue; }
                created++;
            }
            if (f.getName().startsWith("FUN_") || f.getName().startsWith("thunk_FUN_") || f.getName().startsWith("AI2C_")) {
                try { f.setName(primary, SourceType.USER_DEFINED); renamed++; } catch (Exception ex) { println("rename fail " + primary + " " + ex); }
            }
            for (int i = 0; i < l.size() && i < 40; i++) {
                String n = safe(l.get(i));
                if (!n.equals(f.getName())) {
                    try { st.createLabel(addr, n, SourceType.USER_DEFINED); } catch (Exception ex) {}
                }
            }
            namesOut.println(String.format("%08x\t%s\t%s\tai2c", e.getKey(), f.getName(), String.join("|", l)));
        }
        namesOut.close();
        println("KKEXPORT renamed=" + renamed + " created=" + created);

        // strings
        PrintWriter strOut = new PrintWriter(new File(out, "index/strings.tsv"));
        strOut.println("addr\tstring\trefs");
        Map<Address, String> strAt = new HashMap<>();
        for (Data d : currentProgram.getListing().getDefinedData(true)) {
            if (d.getDataType() instanceof StringDataType || d.getDataType() instanceof TerminatedStringDataType || d.getDataType() instanceof UnicodeDataType || d.getDataType() instanceof TerminatedUnicodeDataType) {
                Object v = d.getValue();
                if (v == null) continue;
                String s = v.toString().replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r");
                strAt.put(d.getAddress(), s);
                StringBuilder refs = new StringBuilder();
                for (Reference r : currentProgram.getReferenceManager().getReferencesTo(d.getAddress())) {
                    Function rf = fm.getFunctionContaining(r.getFromAddress());
                    refs.append(rf == null ? r.getFromAddress().toString() : rf.getName()).append(",");
                }
                strOut.println(d.getAddress() + "\t" + s + "\t" + refs);
            }
        }
        strOut.close();

        // decompile everything
        DecompInterface di = new DecompInterface();
        DecompileOptions opts = new DecompileOptions();
        di.setOptions(opts);
        di.toggleCCode(true);
        di.toggleSyntaxTree(false);
        di.setSimplificationStyle("decompile");
        di.openProgram(currentProgram);
        PrintWriter fnOut = new PrintWriter(new File(out, "index/functions.jsonl"));
        PrintWriter cgOut = new PrintWriter(new File(out, "index/callgraph.tsv"));
        cgOut.println("caller\tcallee");
        Gson gson = new Gson();
        int n = 0, fail = 0;
        ConsoleTaskMonitor mon = new ConsoleTaskMonitor();
        for (Function f : fm.getFunctions(true)) {
            n++;
            if (f.isThunk() || f.isExternal()) continue;
            Map<String, Object> rec = new LinkedHashMap<>();
            rec.put("addr", f.getEntryPoint().toString());
            rec.put("name", f.getName());
            long size = 0; for (AddressRange r : f.getBody()) size += r.getLength();
            rec.put("size", size);
            List<String> callers = new ArrayList<>();
            for (Function c : f.getCallingFunctions(mon)) callers.add(c.getName());
            List<String> callees = new ArrayList<>();
            for (Function c : f.getCalledFunctions(mon)) { callees.add(c.getName()); cgOut.println(f.getName() + "\t" + c.getName()); }
            Set<String> strs = new LinkedHashSet<>();
            Set<String> globals = new LinkedHashSet<>();
            for (Reference r : currentProgram.getReferenceManager().getReferenceIterator(f.getEntryPoint())) {
                if (!f.getBody().contains(r.getFromAddress())) { if (r.getFromAddress().compareTo(f.getBody().getMaxAddress()) > 0) break; else continue; }
                Address to = r.getToAddress();
                if (strAt.containsKey(to)) strs.add(strAt.get(to));
                else if (r.getReferenceType().isData() && to.getOffset() >= 0x400000 && to.getOffset() < 0x1000000 && fm.getFunctionContaining(to) == null) globals.add(to.toString());
            }
            rec.put("callers", callers);
            rec.put("callees", callees);
            rec.put("strings", new ArrayList<>(strs));
            rec.put("globals", new ArrayList<>(globals));
            List<String> aliases = new ArrayList<>();
            for (Symbol s : st.getSymbols(f.getEntryPoint())) if (!s.getName().equals(f.getName())) aliases.add(s.getName());
            rec.put("aliases", aliases);
            String code;
            File cf = new File(out, "functions/" + safe(f.getName()) + "_" + f.getEntryPoint() + ".c");
            boolean resume = System.getenv("KK_EXPORT_RESUME") != null;
            if (resume && cf.exists()) {
                // resumable: keep the existing decompilation (header lines are re-generated below)
                StringBuilder sb = new StringBuilder();
                for (String line : Files.readAllLines(cf.toPath())) { if (!line.startsWith("// ")) sb.append(line).append('\n'); }
                code = sb.toString();
            } else try {
                DecompileResults res = di.decompileFunction(f, 60, mon);
                code = (res != null && res.decompileCompleted()) ? res.getDecompiledFunction().getC() : "/* decompile failed: " + (res == null ? "null" : res.getErrorMessage()) + " */\n";
            } catch (Exception ex) { code = "/* decompile exception: " + ex + " */\n"; fail++; }
            String file = "functions/" + safe(f.getName()) + "_" + f.getEntryPoint() + ".c";
            rec.put("file", file);
            try (PrintWriter w = new PrintWriter(new File(out, file))) {
                w.println("// " + f.getName() + " @ " + f.getEntryPoint() + " size=" + size);
                if (!aliases.isEmpty()) w.println("// aliases: " + String.join(", ", aliases));
                w.println("// callers: " + String.join(", ", callers));
                w.println("// callees: " + String.join(", ", callees));
                if (!strs.isEmpty()) w.println("// strings: " + gson.toJson(strs));
                w.print(code);
            }
            fnOut.println(gson.toJson(rec));
            if (n % 500 == 0) { println("KKEXPORT progress " + n); fnOut.flush(); }
        }
        fnOut.close(); cgOut.close();
        di.dispose();
        println("KKEXPORT done functions=" + n + " failed=" + fail);
    }
}
