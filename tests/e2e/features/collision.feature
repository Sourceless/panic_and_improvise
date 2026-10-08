Feature: Solid things, and getting over them

  Background:
    Given I am in test room 1
    And I have the smg
    And the target dummy is out of the way

  Scenario: Collision: a tall wall stops you
    Given there is a 3 metre high wall ahead
    When I hold W
    And I wait 2 seconds
    Then I am still on this side of it
    And I stopped 0.7 metres short of its middle

  Scenario: Collision: a tree trunk stops you
    Given there is a tree trunk ahead
    When I hold W
    And I wait 2 seconds
    Then I am still on this side of it
    And I stopped 0.9 metres short of its middle

  Scenario: Collision: a low kerb is just stepped over
    Given there is a 0.3 metre high wall ahead
    When I hold W
    Then I am on the far side of it
    And I am on the ground

  Scenario: Collision: you can't get over a hedge by just walking at it
    Given there is a 1.5 metre high hedge ahead
    When I hold W
    And I wait 2 seconds
    Then I am still on this side of it
    And I have climbed exactly 0 times

  Scenario: Collision: holding jump gets you over a fence
    Given there is a 1.0 metre high fence ahead
    When I hold W
    And I hold Space
    Then I am on the far side of it

  Scenario: Collision: holding jump gets you over a stone wall
    Given there is a 1.1 metre high wall ahead
    When I hold W
    And I hold Space
    Then I am on the far side of it

  Scenario: Collision: holding jump hauls you over a hedge, and you drop down the far side
    Given there is a 1.5 metre high hedge ahead
    When I hold W
    And I hold Space
    Then I am on the far side of it
    And I have climbed at least 1 times
    When I release Space
    Then I am on the ground

  Scenario: Collision: a wall too tall to reach cannot be climbed
    Given there is a 2.6 metre high wall ahead
    When I hold W
    And I hold Space
    And I wait 2 seconds
    Then I am still on this side of it
    And I have climbed exactly 0 times

  Scenario: Collision: you can climb up onto something and stand on it
    Given there is a platform 1.7 metres high ahead
    When I hold W
    And I hold Space
    Then my feet are at least 1.6 metres up
    When I release W
    And I release Space
    And I wait 1.5 seconds
    Then my feet are 1.7 metres up

  Scenario: Collision: and walk off the far side of it and fall to the ground
    Given there is a platform 1.7 metres high ahead
    When I hold W
    And I hold Space
    Then my feet are at least 1.6 metres up
    When I release Space
    And I wait 3 seconds
    Then I am past z of -8.5
    And I am on the ground
