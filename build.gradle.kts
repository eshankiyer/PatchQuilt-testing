plugins {
    base
}

allprojects {
    group = "org.patchquilt"
    version = "0.1.0"

    repositories {
        mavenCentral()
        maven("https://maven.quiltmc.org/repository/release/")
        maven("https://maven.fabricmc.net/")
    }
}

tasks.register<Exec>("conformanceTest") {
    dependsOn(":host:installDist", ":test-mod:jar")
    val conformanceDirectory = layout.buildDirectory.dir("conformance")
    doFirst {
        val directory = conformanceDirectory.get().asFile
        delete(directory)
        val mods = directory.resolve("mods")
        mods.mkdirs()
        copy {
            from(project(":test-mod").tasks.named("jar"))
            into(mods)
        }
        val libraries = project(":host").layout.buildDirectory.dir("install/host/lib").get().asFile
        val classpath = libraries.listFiles()
            .orEmpty()
            .sortedBy { it.name }
            .joinToString(java.io.File.pathSeparator) { it.absolutePath }
        val marker = directory.resolve("lifecycle.marker")
        commandLine(
            java.nio.file.Path.of(System.getProperty("java.home"), "bin", "java").toString(),
            "-Dpatchquilt.marker=${marker.absolutePath}",
            "-cp",
            classpath,
            "org.patchquilt.host.PatchQuiltLauncher",
            "--gameDir",
            directory.absolutePath,
        )
        standardInput = java.io.ByteArrayInputStream("STOP\n".toByteArray())
    }
    doLast {
        val marker = conformanceDirectory.get().file("lifecycle.marker").asFile
        check(marker.readText() == "patchquilt_lifecycle_test=1.0.0")
    }
}
