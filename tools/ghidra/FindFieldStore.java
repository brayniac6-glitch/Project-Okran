// Functions that store to [reg + <offset>] (the offset as a memory destination), ranked by how many
// of the given offsets they write. Args: <offset hex> [<offset hex> ...]. Finds a struct's writers,
// e.g. the function that fills a camera's matrices.
// @category Kenshi
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;

import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;

public class FindFieldStore extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Set<String> needles = new HashSet<>();
        for (String a : args) {
            needles.add("+ 0x" + Long.toHexString(Long.parseUnsignedLong(a.replace("0x", ""), 16)) + "]");
        }
        Map<Function, Set<String>> found = new HashMap<>();
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction ins = it.next();
            if (ins.getNumOperands() < 2) continue;
            String m = ins.getMnemonicString().toUpperCase();
            if (!(m.startsWith("MOV") || m.startsWith("VMOV"))) continue;
            String dst = ins.getDefaultOperandRepresentation(0).toLowerCase();
            if (!dst.startsWith("[") && !dst.contains("ptr [")) continue;
            for (String n : needles) {
                if (dst.endsWith(n)) {
                    Function f = getFunctionContaining(ins.getAddress());
                    if (f != null) found.computeIfAbsent(f, k -> new HashSet<>()).add(n);
                }
            }
        }
        found.entrySet().stream()
            .sorted((a, b) -> b.getValue().size() - a.getValue().size())
            .limit(40)
            .forEach(e -> println(String.format("%s\t%d\t%d\t%s", e.getKey().getEntryPoint(), e.getValue().size(), e.getKey().getBody().getNumAddresses(), e.getValue())));
    }
}
