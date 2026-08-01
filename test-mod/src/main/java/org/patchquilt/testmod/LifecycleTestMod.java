package org.patchquilt.testmod;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.quiltmc.loader.api.ModContainer;
import org.quiltmc.qsl.base.api.entrypoint.ModInitializer;

public final class LifecycleTestMod implements ModInitializer {
    @Override
    public void onInitialize(ModContainer mod) {
        Path marker = Path.of(System.getProperty("patchquilt.marker"));
        try {
            Files.writeString(marker, mod.metadata().id() + "=" + mod.metadata().version().raw());
        } catch (IOException exception) {
            throw new IllegalStateException(exception);
        }
    }
}
