package org.patchquilt.host;

import net.fabricmc.api.EnvType;
import org.quiltmc.loader.impl.launch.knot.Knot;

public final class PatchQuiltLauncher {
    private PatchQuiltLauncher() {
    }

    public static void main(String[] args) {
        Knot.launch(args, EnvType.SERVER);
    }
}
