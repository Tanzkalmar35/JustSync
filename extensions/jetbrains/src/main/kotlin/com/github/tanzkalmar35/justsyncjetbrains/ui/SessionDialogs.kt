package com.github.tanzkalmar35.justsyncjetbrains.ui

import com.intellij.openapi.ui.DialogWrapper
import com.intellij.ui.dsl.builder.bindText
import com.intellij.ui.dsl.builder.panel
import javax.swing.JComponent

class HostDialog : DialogWrapper(true) {
    var relayAddress: String = "127.0.0.1"
    var password: String = ""

    init {
        title = "Host JustSync Session"
        init()
    }

    override fun createCenterPanel(): JComponent {
        return panel {
            row("Relay Address:") {
                textField().bindText(::relayAddress)
            }
            row("Password:") {
                passwordField().bindText(::password)
            }
        }
    }
}

class JoinDialog : DialogWrapper(true) {
    var relayAddress: String = "127.0.0.1"
    var password: String = ""
    var sessionName: String = ""

    init {
        title = "Join JustSync Session"
        init()
    }

    override fun createCenterPanel(): JComponent {
        return panel {
            row("Relay Address:") {
                textField().bindText(::relayAddress)
            }
            row("Password:") {
                passwordField().bindText(::password)
            }
            row("Session Name:") {
                textField().bindText(::sessionName)
            }
        }
    }
}
