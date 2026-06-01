package com.github.tanzkalmar35.justsyncjetbrains.services

import com.github.tanzkalmar35.justsyncjetbrains.lsp.JustSyncLanguageServer
import com.github.tanzkalmar35.justsyncjetbrains.lsp.JustSyncLspServerSupportProvider
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.EditorFactory
import com.intellij.openapi.editor.event.CaretEvent
import com.intellij.openapi.editor.event.CaretListener
import com.intellij.openapi.editor.markup.HighlighterLayer
import com.intellij.openapi.editor.markup.HighlighterTargetArea
import com.intellij.openapi.editor.markup.RangeHighlighter
import com.intellij.openapi.editor.markup.TextAttributes
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.platform.lsp.api.LspServerState
import com.intellij.platform.lsp.api.LspServerSupportProvider
import java.awt.Color

private val LOG = Logger.getInstance(JustSyncService::class.java)

@Service(Service.Level.PROJECT)
class JustSyncService(private val project: Project) {

    var isRunning = false
    var modeLabel = "Stopped"
    var currentArgs = emptyList<String>()

    // agent_id -> RangeHighlighter
    private val remoteCursors = mutableMapOf<String, RangeHighlighter>()

    init {
        LOG.info("JustSyncService initialized for project: ${project.name}")
    }

    fun startSession(args: List<String>, label: String) {
        LOG.info("startSession request: $label, project path: ${project.basePath}")
        
        val openFiles = FileEditorManager.getInstance(project).openFiles
        if (openFiles.isEmpty()) {
            LOG.warn("No files are currently open. LSP server may not start until a file is opened.")
        } else {
            LOG.info("Currently open files: ${openFiles.map { it.path }}")
        }

        currentArgs = args
        modeLabel = label
        isRunning = true

        ApplicationManager.getApplication().invokeLater {
            try {
                LOG.info("Executing platform server restart...")
                val lspServerManager = LspServerManager.getInstance(project)
                
                // Debug extension points again in EDT
                val providers = LspServerSupportProvider.EP_NAME.extensionList
                LOG.info("EDT: Registered LSP providers: ${providers.map { it::class.java.name }}")

                lspServerManager.stopServers(JustSyncLspServerSupportProvider::class.java)
                lspServerManager.startServersIfNeeded(JustSyncLspServerSupportProvider::class.java)
                LOG.info("EDT: startServersIfNeeded called successfully.")
            } catch (e: Exception) {
                LOG.error("Failed to start LSP servers in EDT", e)
            }
        }
    }

    fun stopSession() {
        LOG.info("Stopping JustSync session")
        isRunning = false
        modeLabel = "Stopped"
        currentArgs = emptyList()

        clearAllCursors()
        LspServerManager.getInstance(project).stopServers(JustSyncLspServerSupportProvider::class.java)
    }

    fun updateRemoteCursor(agentId: String, uri: String, line: Int, character: Int) {
        ApplicationManager.getApplication().invokeLater {
            val virtualFile = VirtualFileManager.getInstance().findFileByUrl(uri) ?: return@invokeLater
            val document = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return@invokeLater
            val editors = EditorFactory.getInstance().getEditors(document, project)
            if (editors.isEmpty()) return@invokeLater
            
            val editor = editors[0]
            val offset = editor.document.getLineStartOffset(line) + character
            
            // Remove old highlighter if it exists
            remoteCursors[agentId]?.let { editor.markupModel.removeHighlighter(it) }

            // Create new highlighter (Blue bar)
            val attributes = TextAttributes().apply {
                backgroundColor = Color(49, 116, 143, 128)
                effectColor = Color(49, 116, 143)
                effectType = com.intellij.openapi.editor.markup.EffectType.LINE_UNDERSCORE
            }

            val highlighter = editor.markupModel.addRangeHighlighter(
                offset, offset + 1,
                HighlighterLayer.LAST + 100,
                attributes,
                HighlighterTargetArea.EXACT_RANGE
            )
            
            remoteCursors[agentId] = highlighter
        }
    }

    private fun clearAllCursors() {
        ApplicationManager.getApplication().invokeLater {
            remoteCursors.values.forEach { highlighter ->
                // In a real scenario, we'd need to find which editor the highlighter belongs to.
                // For simplicity in this alpha, we clear the map. 
                // A more robust implementation would track the editor as well.
            }
            remoteCursors.clear()
        }
    }
}
