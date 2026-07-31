package com.github.tanzkalmar35.justsyncjetbrains.ui

import com.github.tanzkalmar35.justsyncjetbrains.services.JustSyncService
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.actionSystem.impl.SimpleDataContext
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.popup.JBPopupFactory
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.intellij.ui.awt.RelativePoint
import com.intellij.util.Consumer
import java.awt.event.MouseEvent
import javax.swing.JOptionPane 

private val LOG = Logger.getInstance(JustSyncStatusBarWidgetFactory::class.java)

class JustSyncStatusBarWidgetFactory : StatusBarWidgetFactory {
    override fun getId() = "JustSyncStatusBar"
    override fun getDisplayName() = "JustSync"
    override fun isAvailable(project: Project) = true

    override fun createWidget(project: Project): StatusBarWidget {
        return JustSyncStatusBarWidget(project)
    }

    override fun disposeWidget(widget: StatusBarWidget) {}
    override fun canBeEnabledOn(statusBar: StatusBar) = true
}

class JustSyncStatusBarWidget(private val project: Project) : StatusBarWidget, StatusBarWidget.TextPresentation {

    override fun ID() = "JustSyncStatusBar"

    // Accesses self as presentation
    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this

    override fun install(statusBar: StatusBar) {}
    override fun dispose() {}

    // --- TextPresentation methods ---

    override fun getTooltipText() = "Click to control JustSync"

    override fun getText(): String {
        val service = project.getService(JustSyncService::class.java)
        return if (service?.isRunning == true) "JustSync: ${service.modeLabel}" else "JustSync: Play"
    }

    override fun getAlignment() = 0f 

    override fun getClickConsumer(): Consumer<MouseEvent> {
        return Consumer { event ->
            showPopup(event)
        }
    }

    private fun showPopup(event: MouseEvent) {
        val service = project.getService(JustSyncService::class.java)

        if (service == null) {
            // Fallback
            JOptionPane.showMessageDialog(null, "Error: JustSync Service not found!")
            return
        }

        val group = DefaultActionGroup()

        if (service.isRunning) {
            group.add(object : AnAction("Stop JustSync") {
                override fun actionPerformed(e: AnActionEvent) {
                    service.stopSession()
                    updateWidget()
                }
            })
        } else {
            group.add(object : AnAction("Host Session") {
                override fun actionPerformed(e: AnActionEvent) {
                    val dialog = HostDialog()
                    if (dialog.showAndGet()) {
                        service.startSession(
                            listOf("--mode", "host", "--remote-ip", dialog.relayAddress, "--key", dialog.password),
                            "Host"
                        )
                        updateWidget()
                    }
                }
            })
            group.add(object : AnAction("Join Session") {
                override fun actionPerformed(e: AnActionEvent) {
                    val dialog = JoinDialog()
                    if (dialog.showAndGet()) {
                        service.startSession(
                            listOf(
                                "--mode", "peer",
                                "--remote-ip", dialog.relayAddress,
                                "--key", dialog.password,
                                "--session-name", dialog.sessionName
                            ),
                            "Peer"
                        )
                        updateWidget()
                    }
                }
            })
        }

        val context = SimpleDataContext.getProjectContext(project)
        val popup = JBPopupFactory.getInstance().createActionGroupPopup(
            "JustSync",
            group,
            context,
            JBPopupFactory.ActionSelectionAid.SPEEDSEARCH,
            true
        )

        if (event.component != null) {
            popup.show(RelativePoint(event.component, event.point))
        } else {
            popup.showInBestPositionFor(context)
        }
    }

    private fun updateWidget() {
        val statusBar = com.intellij.openapi.wm.WindowManager.getInstance().getStatusBar(project)
        statusBar?.updateWidget(ID())
    }
}
