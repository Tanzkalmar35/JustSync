package com.github.tanzkalmar35.justsyncjetbrains.lsp

import com.github.tanzkalmar35.justsyncjetbrains.services.JustSyncService
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor

class JustSyncLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter
    ) {
        val service = project.getService(JustSyncService::class.java)

        // Nothing to do if the user hasn't started the service
        if (!service.isRunning) {
            return
        }

        // Start server if it's a supported file
        if (isSupportedFile(file)) {
            serverStarter.ensureServerStarted(JustSyncLspDescriptor(project, service.currentArgs))
        }
    }

    // Filter unwanted file types
    // For now all are allowed...
    private fun isSupportedFile(file: VirtualFile): Boolean {
        return true
    }
}
class JustSyncLspDescriptor(project: Project, private val args: List<String>) : ProjectWideLspServerDescriptor(project, "JustSync") {

    override fun isSupportedFile(file: VirtualFile) = true

    override fun createCommandLine(): GeneralCommandLine {
        return GeneralCommandLine().apply {
            // Annahme: "justsync" ist im PATH
            exePath = "/home/fabian/Desktop/git/JustSync/target/release/JustSync"
            addParameters(args)
            setWorkDirectory(project.basePath)
        }
    }

    override val lspGoToDefinitionSupport = false
    override val lspCompletionSupport = null
}
