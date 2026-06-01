package com.github.tanzkalmar35.justsyncjetbrains.lsp

import com.github.tanzkalmar35.justsyncjetbrains.services.JustSyncService
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.Lsp4jClient
import com.intellij.platform.lsp.api.LspServerNotificationsHandler
import org.eclipse.lsp4j.services.LanguageServer
import org.eclipse.lsp4j.services.LanguageClient
import org.eclipse.lsp4j.jsonrpc.services.JsonNotification

private val LOG = Logger.getInstance(JustSyncLspServerSupportProvider::class.java).also {
    it.info("JustSyncLspServerSupportProvider class loaded into memory")
}

class JustSyncLspServerSupportProvider : LspServerSupportProvider {
    init {
        println("JustSync: JustSyncLspServerSupportProvider instance created: ${this.hashCode()}")
    }

    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter
    ) {
        println("JustSync: fileOpened called for: ${file.path}")
        val service = project.getService(JustSyncService::class.java)

        if (!service.isRunning) {
            println("JustSync: Service is not running, skipping server start for: ${file.path}")
            return
        }

        println("JustSync: Ensuring server started for file: ${file.path}")
        serverStarter.ensureServerStarted(JustSyncLspDescriptor(project, service.currentArgs))
    }
}

class JustSyncLspDescriptor(project: Project, private val args: List<String>) : ProjectWideLspServerDescriptor(project, "JustSync") {

    override fun isSupportedFile(file: VirtualFile): Boolean {
        println("JustSync: isSupportedFile called for: ${file.path}")
        return true
    }

    override fun createCommandLine(): GeneralCommandLine {
        println("JustSync: createCommandLine called with args: $args")
        
        var binaryPath = "just_sync"
        
        // Try to find binary in project target directory or common locations
        val projectPath = project.basePath
        val possiblePaths = mutableListOf<String>()
        if (projectPath != null) {
            possiblePaths.addAll(listOf(
                "target/release/just_sync",
                "target/debug/just_sync",
                "bin/just_sync"
            ).map { java.io.File(projectPath, it).absolutePath })
        }
        possiblePaths.add("/usr/local/bin/just_sync")
        possiblePaths.add("/usr/bin/just_sync")

        for (path in possiblePaths) {
            val file = java.io.File(path)
            if (file.exists() && file.canExecute()) {
                binaryPath = file.absolutePath
                println("JustSync: Found binary at: $binaryPath")
                break
            }
        }

        if (binaryPath == "just_sync") {
            println("JustSync: WARN - Could not find just_sync binary in known locations. Falling back to system PATH.")
        }

        return GeneralCommandLine().apply {
            exePath = binaryPath
            addParameters(args)
            addParameter("--stdio")
            setWorkDirectory(project.basePath)
            println("JustSync: Executing command: $commandLineString")
        }
    }

    override val lspGoToDefinitionSupport = false
    override val lspCompletionSupport = null

    override val lsp4jServerClass: Class<out LanguageServer> = JustSyncLanguageServer::class.java

    override fun createLsp4jClient(handler: LspServerNotificationsHandler): Lsp4jClient {
        return JustSyncLsp4jClient(project, handler)
    }
}

interface JustSyncLanguageServer : LanguageServer {
    @JsonNotification("$/justsync/cursor")
    fun cursor(params: Map<String, Any>)
}

interface JustSyncLanguageClient : LanguageClient {
    @JsonNotification("$/justsync/remoteCursor")
    fun remoteCursor(params: Map<String, Any>)
}

class JustSyncLsp4jClient(private val project: Project, handler: LspServerNotificationsHandler) : Lsp4jClient(handler), JustSyncLanguageClient {
    override fun remoteCursor(params: Map<String, Any>) {
        val agentId = params["agent_id"] as? String ?: return
        val uri = params["uri"] as? String ?: return
        val pos = params["position"] as? Map<*, *> ?: return
        val line = (pos["line"] as? Double)?.toInt() ?: return
        val character = (pos["character"] as? Double)?.toInt() ?: return

        project.getService(JustSyncService::class.java).updateRemoteCursor(agentId, uri, line, character)
    }
}
