package org.patchquilt.host;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import net.fabricmc.api.DedicatedServerModInitializer;
import net.fabricmc.api.ModInitializer;
import org.quiltmc.loader.api.QuiltLoader;
import org.quiltmc.loader.api.entrypoint.EntrypointContainer;

public final class PatchQuiltRuntime {
    private PatchQuiltRuntime() {
    }

    public static void main(String[] args) {
        for (EntrypointContainer<org.quiltmc.qsl.base.api.entrypoint.ModInitializer> entrypoint :
                QuiltLoader.getEntrypointContainers("init", org.quiltmc.qsl.base.api.entrypoint.ModInitializer.class)) {
            entrypoint.getEntrypoint().onInitialize(entrypoint.getProvider());
        }
        for (EntrypointContainer<org.quiltmc.qsl.base.api.entrypoint.server.DedicatedServerModInitializer> entrypoint :
                QuiltLoader.getEntrypointContainers("server_init", org.quiltmc.qsl.base.api.entrypoint.server.DedicatedServerModInitializer.class)) {
            entrypoint.getEntrypoint().onInitializeServer(entrypoint.getProvider());
        }
        for (ModInitializer entrypoint : QuiltLoader.getEntrypoints("main", ModInitializer.class)) {
            entrypoint.onInitialize();
        }
        for (DedicatedServerModInitializer entrypoint : QuiltLoader.getEntrypoints("server", DedicatedServerModInitializer.class)) {
            entrypoint.onInitializeServer();
        }
        System.out.println("PATCHQUILT_READY");
        try {
            BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
            while (true) {
                String command = reader.readLine();
                if (command == null || command.equals("STOP")) {
                    return;
                }
            }
        } catch (IOException exception) {
            throw new IllegalStateException(exception);
        }
    }
}
