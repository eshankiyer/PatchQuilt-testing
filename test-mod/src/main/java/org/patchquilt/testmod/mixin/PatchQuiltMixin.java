package org.patchquilt.testmod.mixin;

import org.patchquilt.host.PatchQuiltMixinProbe;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(PatchQuiltMixinProbe.class)
public final class PatchQuiltMixin {
    @Inject(method = "value", at = @At("HEAD"), cancellable = true)
    private static void patchquilt$value(CallbackInfoReturnable<String> callback) {
        callback.setReturnValue("mixed");
    }
}
