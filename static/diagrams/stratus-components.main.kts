// kotlin stratus-components.main.kts

// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------
@file:DependsOn("com.structurizr:structurizr-core:2.2.0")
@file:DependsOn("com.structurizr:structurizr-export:2.2.0")

import com.structurizr.*
import com.structurizr.export.*
import com.structurizr.export.mermaid.*
import com.structurizr.model.*
import com.structurizr.view.*
import java.io.File

// -----------------------------------------------------------------------------
// Setup
// -----------------------------------------------------------------------------
val workspace = Workspace("Stratus", null)
val views = workspace.views
val model = workspace.model
val stratus = model.addSoftwareSystem("Stratus")

// -----------------------------------------------------------------------------
// Components
// -----------------------------------------------------------------------------
val core = stratus.addContainer("Core")
val storage = stratus.addContainer("Storage")

// -----------------------------------------------------------------------------
// Views
// -----------------------------------------------------------------------------
val stratusView = views.createSystemContextView(stratus, "Internal components", "")
stratusView.addAllElements()

// -----------------------------------------------------------------------------
// Exporter
// -----------------------------------------------------------------------------
val diagram = MermaidDiagramExporter().export(stratusView)
File("stratus-components.md").writeText("```mermaid\n${diagram.definition}\n```")
