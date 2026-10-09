// Instructions whose text contains a pattern (e.g. "+ 0x1d4]"), optionally within an address range.
// Args: <pattern> [<start hex> <end hex>]. Prints function, address, instruction.
// @category Kenshi
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;

public class FindOperand extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String needle = args[0].toLowerCase();
        InstructionIterator it;
        if (args.length >= 3) {
            Address a = toAddr(Long.parseUnsignedLong(args[1].replace("0x", ""), 16));
            Address b = toAddr(Long.parseUnsignedLong(args[2].replace("0x", ""), 16));
            it = currentProgram.getListing().getInstructions(new AddressSet(a, b), true);
        } else {
            it = currentProgram.getListing().getInstructions(true);
        }
        int hits = 0;
        while (it.hasNext() && !monitor.isCancelled() && hits < 300) {
            Instruction ins = it.next();
            if (ins.toString().toLowerCase().contains(needle)) {
                Function f = getFunctionContaining(ins.getAddress());
                println(String.format("%s\t%s\t%s", f == null ? "?" : f.getEntryPoint(), ins.getAddress(), ins));
                hits++;
            }
        }
        println("hits=" + hits);
    }
}
