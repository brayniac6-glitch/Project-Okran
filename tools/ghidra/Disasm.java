// Print instructions from an address, disassembling where analysis left bytes undefined (for
// functions the decompiler mangles). Args: <hex address> <max bytes> <out file>. Walks linearly and
// stops at the first RET after <max bytes>. Output is raw (local only, decomp/).
// @category Kenshi
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;

import java.io.File;
import java.io.PrintWriter;

public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address start = toAddr(args[0]);
        long max = Long.parseLong(args[1]);
        try (PrintWriter w = new PrintWriter(new File(args[2]), "UTF-8")) {
            Address a = start;
            while (true) {
                Instruction i = getInstructionAt(a);
                if (i == null) {
                    disassemble(a);
                    i = getInstructionAt(a);
                }
                if (i == null) {
                    w.println(a + "  ?? (cannot disassemble)");
                    break;
                }
                w.println(i.getAddress() + "  " + i);
                a = i.getMaxAddress().next();
                if (a.subtract(start) >= max && i.getMnemonicString().startsWith("RET")) {
                    break;
                }
                if (a.subtract(start) > max + 256) {
                    break;
                }
            }
        }
    }
}
