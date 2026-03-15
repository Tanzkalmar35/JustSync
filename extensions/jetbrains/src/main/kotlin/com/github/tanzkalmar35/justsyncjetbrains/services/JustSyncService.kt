package com.github.tanzkalmar35.justsyncjetbrains.services

import com.intellij.openapi.components.Service
import com.intellij.openapi.project.Project
import com.github.tanzkalmar35.justsyncjetbrains.lsp.JustSyncLspServerSupportProvider
import com.intellij.platform.lsp.api.LspServerManager

@Service(Service.Level.PROJECT)
class JustSyncService(private val project: Project) {

    var isRunning = false
    var modeLabel = "Stopped"
    var currentArgs = emptyList<String>();

    fun startSession(args: List<String>, label: String) {
        currentArgs = args
        modeLabel = label
        isRunning = true

        // Force lsp restart
        LspServerManager.getInstance(project).stopServers(JustSyncLspServerSupportProvider::class.java)
        LspServerManager.getInstance(project).startServersIfNeeded(JustSyncLspServerSupportProvider::class.java)
    }

    fun stopSession() {
        isRunning = false
        modeLabel = "Stopped"
        currentArgs = emptyList()

        // stop lsp
        LspServerManager.getInstance(project).stopServers(JustSyncLspServerSupportProvider::class.java)
    }
}
