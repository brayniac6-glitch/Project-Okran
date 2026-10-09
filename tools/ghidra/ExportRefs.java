// Export the call graph and string references for subsystem triage (sheet.py ingest).
// Output (TSV, raw, local only): <outdir>/calls.tsv (caller, callee; thunks resolved) and
// <outdir>/strrefs.tsv (fn, string addr).
// @category Kenshi
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;

import java.io.File;
import java.io.PrintWriter;
import java.util.HashSet;
import java.util.Set;

public class ExportRefs extends GhidraScript {
    private Function resolve(Function f) {
        if (f != null && f.isThunk()) {
            Function t = f.getThunkedFunction(true);
            if (t != null) return t;
        }
        return f;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args.length > 0 ? args[0] : ".");
        outDir.mkdirs();
        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();
        Set<String> calls = new HashSet<>(), strs = new HashSet<>();

        ReferenceIterator it = currentProgram.getReferenceManager().getReferenceIterator(currentProgram.getMinAddress());
        while (it.hasNext() && !monitor.isCancelled()) {
            Reference r = it.next();
            if (!r.getToAddress().isMemoryAddress()) continue;
            Function from = resolve(fm.getFunctionContaining(r.getFromAddress()));
            if (from == null) continue;
            if (r.getReferenceType().isCall()) {
                Function to = resolve(fm.getFunctionAt(r.getToAddress()));
                if (to != null && !to.equals(from)) calls.add(from.getEntryPoint() + "\t" + to.getEntryPoint());
            } else if (r.getReferenceType().isData()) {
                Data d = listing.getDataAt(r.getToAddress());
                if (d != null && d.hasStringValue()) strs.add(from.getEntryPoint() + "\t" + r.getToAddress());
            }
        }
        try (PrintWriter w = new PrintWriter(new File(outDir, "calls.tsv"), "UTF-8")) {
            w.println("caller\tcallee");
            for (String s : calls) w.println(s);
        }
        try (PrintWriter w = new PrintWriter(new File(outDir, "strrefs.tsv"), "UTF-8")) {
            w.println("fn\tstr");
            for (String s : strs) w.println(s);
        }
        println(String.format("calls=%d strrefs=%d", calls.size(), strs.size()));
    }
}
