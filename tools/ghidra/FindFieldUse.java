// Functions that load a global pointer and then use a given field offset within a few instructions
// (e.g. "[[141715F90] + 0x5E0]"). Args: <global hex> <field offset hex> [window]. Prints fn, site, text.
// @category Kenshi
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.symbol.Reference;

public class FindFieldUse extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address global = toAddr(Long.parseUnsignedLong(args[0].replace("0x", ""), 16));
        String field = "0x" + Long.toHexString(Long.parseUnsignedLong(args[1].replace("0x", ""), 16));
        int window = args.length > 2 ? Integer.parseInt(args[2]) : 8;
        Listing listing = currentProgram.getListing();
        int hits = 0;
        for (Reference ref : getReferencesTo(global)) {
            Instruction ins = listing.getInstructionAt(ref.getFromAddress());
            for (int i = 0; ins != null && i < window; i++, ins = ins.getNext()) {
                String text = ins.toString().toLowerCase();
                if (text.contains("+ " + field + "]") || text.contains("+" + field + "]")) {
                    Function f = getFunctionContaining(ins.getAddress());
                    println(String.format("%s\t%s\t%s", f == null ? "?" : f.getEntryPoint(), ins.getAddress(), ins));
                    hits++;
                    break;
                }
            }
        }
        println("hits=" + hits);
    }
}
