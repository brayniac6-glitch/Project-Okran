// Export RTTI class -> vtable -> virtual function slots, plus any function Ghidra already placed in a class
// namespace. Output (TSV, raw, local only): <outdir>/rtti_vtables.tsv and <outdir>/rtti_fns.tsv
// @category Kenshi
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.Namespace;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;
import ghidra.program.model.symbol.SymbolTable;

import java.io.File;
import java.io.PrintWriter;

public class ExportClasses extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args.length > 0 ? args[0] : ".");
        outDir.mkdirs();
        SymbolTable st = currentProgram.getSymbolTable();
        FunctionManager fm = currentProgram.getFunctionManager();
        Memory mem = currentProgram.getMemory();
        int ptr = currentProgram.getDefaultPointerSize();

        int tables = 0, slots = 0;
        try (PrintWriter vt = new PrintWriter(new File(outDir, "rtti_vtables.tsv"), "UTF-8")) {
            vt.println("class\tvtable\tslot\tfn");
            // Ghidra's RTTI analyzer names them "vftable" (older versions: "`vftable'"), inside the class namespace
            SymbolIterator it = st.getSymbolIterator(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Symbol s = it.next();
                String n = s.getName();
                if (!n.equals("vftable") && !n.equals("`vftable'")) continue;
                Address a = s.getAddress();
                if (!a.isMemoryAddress()) continue;
                Namespace ns = s.getParentNamespace();
                if (ns == null || ns.isGlobal()) continue;
                String cls = ns.getName(true);
                tables++;
                for (int i = 0; i < 2048; i++) {
                    Address slot = a.add((long) i * ptr);
                    // stop at the next labelled item (next vtable / RTTI locator)
                    if (i > 0 && st.getPrimarySymbol(slot) != null) break;
                    long target;
                    try { target = ptr == 8 ? mem.getLong(slot) : (mem.getInt(slot) & 0xffffffffL); }
                    catch (Exception e) { break; }
                    Address t;
                    try { t = a.getNewAddress(target); } catch (Exception e) { break; }
                    Function f = fm.getFunctionAt(t);
                    if (f == null) break;
                    // incremental linking: slots point at ILT jmp stubs; record the real body
                    if (f.isThunk() && f.getThunkedFunction(true) != null) f = f.getThunkedFunction(true);
                    vt.printf("%s\t%s\t%d\t%s%n", cls, a, i, f.getEntryPoint());
                    slots++;
                }
            }
        }

        int placed = 0;
        try (PrintWriter fn = new PrintWriter(new File(outDir, "rtti_fns.tsv"), "UTF-8")) {
            fn.println("fn\tns\tname");
            for (Function f : fm.getFunctions(true)) {
                Namespace ns = f.getParentNamespace();
                if (ns == null || ns.isGlobal()) continue;
                fn.printf("%s\t%s\t%s%n", f.getEntryPoint(), ns.getName(true), f.getName());
                placed++;
            }
        }
        println(String.format("vtables=%d slots=%d namespaced_fns=%d", tables, slots, placed));
    }
}
